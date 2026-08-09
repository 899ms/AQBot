//! ACP workbench Tauri commands.

use crate::AppState;
use aqbot_acp_client::config::{
    enabled_agents, is_agent_enabled, load_agents_file, migrate_agents_file, probe_agent,
    remove_agent, reorder_agents, save_agents_file, set_agent_enabled,
    sync_configured_registry_agents, upsert_from_registry, AcpAgentsFile, AcpGeneralConfig,
    ConfiguredAgent,
};
use aqbot_acp_client::proxy::{
    configured_agent_with_proxy, resolve_proxy_environment, resolve_system_proxy,
    ProcessProxySettings,
};
use aqbot_acp_client::registry::{
    find_registry_agent, load_registry, refresh_registry_with_proxy, resolve_launch, RegistryFile,
    RegistrySource,
};
use aqbot_acp_client::runtime::{
    configured_agent_with_model, configured_agent_with_reasoning_effort, persisted_mode_id,
    AcpEvent, AcpInteractionKind, AcpInteractionOutcome, AcpQuestionnaireAnswer,
    AcpQuestionnaireOutcome, AcpQuestionnaireSubmission, AcpRuntime, AcpSessionSnapshot,
    RuntimeLimits,
};
use aqbot_acp_client::types::AgentProbeResult;
use aqbot_acp_client::{AcpPromptAttachment, AcpPromptInput};
use aqbot_core::repo::acp as acp_repo;
use aqbot_core::types::{AppSettings, Attachment, AttachmentInput};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU64, Ordering as AtomicOrdering},
    Arc,
};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{mpsc, Mutex};

/// Process-wide ACP runtime (permission channels + future process pool).
static ACP_RUNTIME: std::sync::OnceLock<Arc<AcpRuntime>> = std::sync::OnceLock::new();
static ACP_CONFIG_LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
static ACP_RECENT_DRAFT_LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
static ACP_LAUNCH_CONFIG_GENERATION: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedAcpToolCall {
    tool_call_id: String,
    tool_name: String,
    status: String,
    input: Option<String>,
    output: Option<String>,
    approval_status: Option<String>,
    approval_option_id: Option<String>,
    approval_option_kind: Option<String>,
    approval_label: Option<String>,
    #[serde(skip)]
    sequence: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpRecentThreadReceipt {
    project: aqbot_core::entity::acp_projects::Model,
    thread: aqbot_core::entity::acp_threads::Model,
}

fn allocate_recent_workspace_path(settings: &AppSettings) -> Result<(PathBuf, String), String> {
    let workspace_id = aqbot_core::utils::gen_id();
    let created_at = chrono::Utc::now().timestamp();
    let workspace_dir =
        super::agent::resolve_agent_workspace_dir_for(settings, &workspace_id, created_at);
    let root_path = workspace_dir
        .to_str()
        .ok_or_else(|| "invalid ACP workspace path encoding".to_string())?
        .to_string();
    Ok((workspace_dir, root_path))
}

async fn create_recent_workspace_project(
    state: &AppState,
    settings: &AppSettings,
    title: &str,
    draft: bool,
) -> Result<aqbot_core::entity::acp_projects::Model, String> {
    let (workspace_dir, root_path) = allocate_recent_workspace_path(settings)?;
    let project = if draft {
        acp_repo::create_recent_draft_workspace(&state.sea_db, title, &root_path).await
    } else {
        acp_repo::create_recent_workspace(&state.sea_db, title, &root_path).await
    }
    .map_err(|error| error.to_string())?;
    if let Err(error) = std::fs::create_dir_all(&workspace_dir) {
        let rollback = acp_repo::delete_project(&state.sea_db, &project.id).await;
        return Err(match rollback {
            Ok(()) => format!("failed to create ACP workspace: {error}"),
            Err(rollback) => {
                format!("failed to create ACP workspace: {error}; rollback failed: {rollback}")
            }
        });
    }
    Ok(project)
}

async fn reusable_recent_draft(
    db: &sea_orm::DatabaseConnection,
) -> Result<Option<aqbot_core::entity::acp_projects::Model>, String> {
    let occupied_projects = acp_repo::list_all_threads(db)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|thread| thread.project_id)
        .collect::<HashSet<_>>();
    Ok(acp_repo::list_projects(db)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|project| project.kind == "recent_draft" && !occupied_projects.contains(&project.id)))
}

#[cfg(test)]
mod recent_draft_tests {
    use super::*;

    #[tokio::test]
    async fn only_an_explicit_unoccupied_recent_draft_is_reusable() {
        let db = aqbot_core::db::create_test_pool().await.unwrap().conn;
        let residual = acp_repo::create_recent_workspace(&db, "Deleted conversation", "/tmp/old")
            .await
            .unwrap();
        let draft =
            acp_repo::create_recent_draft_workspace(&db, "New conversation", "/tmp/recent-draft")
                .await
                .unwrap();

        assert_eq!(
            reusable_recent_draft(&db).await.unwrap().unwrap().id,
            draft.id
        );
        acp_repo::create_thread(&db, &draft.id, "codex", "Claimed")
            .await
            .unwrap();

        assert!(reusable_recent_draft(&db).await.unwrap().is_none());
        assert_eq!(residual.kind, "recent");
    }
}

fn next_tool_sequence(
    tools: &HashMap<String, PersistedAcpToolCall>,
    tool_call_id: &str,
    next_sequence: &mut u64,
) -> u64 {
    tools.get(tool_call_id).map_or_else(
        || {
            let sequence = *next_sequence;
            *next_sequence += 1;
            sequence
        },
        |tool| tool.sequence,
    )
}

fn record_tool_call(
    tools: &mut HashMap<String, PersistedAcpToolCall>,
    next_sequence: &mut u64,
    tool_call_id: &str,
    title: &Option<String>,
    kind: &Option<String>,
    status: &Option<String>,
    raw: &serde_json::Value,
) {
    let sequence = next_tool_sequence(tools, tool_call_id, next_sequence);
    let previous = tools.remove(tool_call_id);
    let tool_name = kind
        .clone()
        .or_else(|| title.clone())
        .or_else(|| previous.as_ref().map(|tool| tool.tool_name.clone()))
        .unwrap_or_else(|| "tool".into());
    let status = status
        .clone()
        .or_else(|| previous.as_ref().map(|tool| tool.status.clone()))
        .unwrap_or_else(|| "queued".into());
    let input =
        tool_input_detail(raw).or_else(|| previous.as_ref().and_then(|tool| tool.input.clone()));
    let output =
        tool_output_detail(raw).or_else(|| previous.as_ref().and_then(|tool| tool.output.clone()));
    tools.insert(
        tool_call_id.to_string(),
        PersistedAcpToolCall {
            tool_call_id: tool_call_id.to_string(),
            tool_name,
            status,
            input,
            output,
            approval_status: previous
                .as_ref()
                .and_then(|tool| tool.approval_status.clone()),
            approval_option_id: previous
                .as_ref()
                .and_then(|tool| tool.approval_option_id.clone()),
            approval_option_kind: previous
                .as_ref()
                .and_then(|tool| tool.approval_option_kind.clone()),
            approval_label: previous
                .as_ref()
                .and_then(|tool| tool.approval_label.clone()),
            sequence,
        },
    );
}

fn record_interaction_outcome(
    tools: &mut HashMap<String, PersistedAcpToolCall>,
    next_sequence: &mut u64,
    tool_call_id: &str,
    interaction_kind: AcpInteractionKind,
    outcome: AcpInteractionOutcome,
    option_id: Option<&str>,
    option_kind: Option<&str>,
    option_label: Option<&str>,
) {
    let sequence = next_tool_sequence(tools, tool_call_id, next_sequence);
    let tool = tools
        .entry(tool_call_id.to_string())
        .or_insert_with(|| PersistedAcpToolCall {
            tool_call_id: tool_call_id.to_string(),
            tool_name: "tool".into(),
            status: "queued".into(),
            input: None,
            output: None,
            approval_status: None,
            approval_option_id: None,
            approval_option_kind: None,
            approval_label: None,
            sequence,
        });
    if interaction_kind != AcpInteractionKind::Permission {
        if outcome == AcpInteractionOutcome::Selected && tool.output.is_none() {
            tool.output = option_label
                .filter(|label| !label.is_empty())
                .map(str::to_owned)
                .or_else(|| option_id.map(|id| format!("aqbot:questionnaire:{id}")));
        }
        return;
    }

    let approval_status = match outcome {
        AcpInteractionOutcome::Selected
            if option_kind.is_some_and(|kind| {
                matches!(
                    kind.to_ascii_lowercase().as_str(),
                    "allowonce" | "allow_once" | "allowalways" | "allow_always"
                )
            }) =>
        {
            "approved"
        }
        AcpInteractionOutcome::Selected => "denied",
        AcpInteractionOutcome::Cancelled => "cancelled",
        AcpInteractionOutcome::Expired => "expired",
    };
    tool.approval_status = Some(approval_status.into());
    tool.approval_option_id = option_id.map(str::to_owned);
    tool.approval_option_kind = option_kind.map(str::to_owned);
    tool.approval_label = option_label.map(str::to_owned);
    if approval_status != "approved" {
        tool.status = "cancelled".into();
    }
}

fn finalize_unfinished_tool_calls(
    tools: &mut HashMap<String, PersistedAcpToolCall>,
    terminal_status: &str,
) {
    for tool in tools.values_mut() {
        let status = tool.status.to_ascii_lowercase();
        let terminal = matches!(
            status.as_str(),
            "completed" | "success" | "failed" | "error" | "cancelled" | "canceled"
        );
        if !terminal {
            tool.status = terminal_status.to_string();
        }
    }
}

#[cfg(test)]
mod tool_transcript_tests {
    use super::*;

    #[test]
    fn permission_outcome_survives_a_later_tool_call_event() {
        let mut tools = HashMap::new();
        let mut next_sequence = 0;
        record_interaction_outcome(
            &mut tools,
            &mut next_sequence,
            "tool-1",
            AcpInteractionKind::Permission,
            AcpInteractionOutcome::Selected,
            Some("allow-once"),
            Some("AllowOnce"),
            Some("Allow once"),
        );

        record_tool_call(
            &mut tools,
            &mut next_sequence,
            "tool-1",
            &Some("Run command".into()),
            &Some("execute".into()),
            &Some("running".into()),
            &serde_json::json!({ "rawInput": { "command": "pwd" } }),
        );

        let tool = tools.get("tool-1").expect("merged tool call");
        assert_eq!(tool.approval_status.as_deref(), Some("approved"));
        assert_eq!(tool.approval_option_id.as_deref(), Some("allow-once"));
        assert_eq!(tool.approval_option_kind.as_deref(), Some("AllowOnce"));
        assert_eq!(tool.approval_label.as_deref(), Some("Allow once"));
        assert_eq!(tool.tool_name, "execute");
        assert_eq!(tool.sequence, 0);
        assert_eq!(next_sequence, 1);

        let serialized = serde_json::to_value(tool).expect("serialize persisted tool");
        assert_eq!(serialized["approvalStatus"], "approved");
        assert_eq!(serialized["approvalOptionId"], "allow-once");
        assert_eq!(serialized["approvalOptionKind"], "AllowOnce");
        assert_eq!(serialized["approvalLabel"], "Allow once");
    }

    #[test]
    fn permission_terminal_outcomes_keep_their_meaning() {
        for (outcome, kind, expected) in [
            (AcpInteractionOutcome::Cancelled, None, "cancelled"),
            (AcpInteractionOutcome::Expired, None, "expired"),
            (
                AcpInteractionOutcome::Selected,
                Some("RejectOnce"),
                "denied",
            ),
        ] {
            let mut tools = HashMap::new();
            let mut next_sequence = 0;
            record_interaction_outcome(
                &mut tools,
                &mut next_sequence,
                "tool-1",
                AcpInteractionKind::Permission,
                outcome,
                Some("deny"),
                kind,
                Some("Deny"),
            );
            assert_eq!(tools["tool-1"].approval_status.as_deref(), Some(expected));
            assert_eq!(tools["tool-1"].status, "cancelled");
        }
    }

    #[test]
    fn question_and_plan_outcomes_preserve_answers_until_the_agent_finishes_the_tool() {
        for interaction_kind in [AcpInteractionKind::Question, AcpInteractionKind::PlanReview] {
            let mut tools = HashMap::new();
            let mut next_sequence = 0;
            record_interaction_outcome(
                &mut tools,
                &mut next_sequence,
                "tool-1",
                interaction_kind,
                AcpInteractionOutcome::Selected,
                Some("choice-1"),
                None,
                Some("Use SQLite"),
            );

            assert_eq!(tools["tool-1"].status, "queued");
            assert_eq!(tools["tool-1"].output.as_deref(), Some("Use SQLite"));
            assert_eq!(tools["tool-1"].approval_status, None);
        }
    }

    #[test]
    fn empty_plan_action_persists_its_semantic_result_id() {
        let mut tools = HashMap::new();
        let mut next_sequence = 0;

        record_interaction_outcome(
            &mut tools,
            &mut next_sequence,
            "tool-1",
            AcpInteractionKind::PlanReview,
            AcpInteractionOutcome::Selected,
            Some("skip_interview"),
            None,
            Some(""),
        );

        assert_eq!(
            tools["tool-1"].output.as_deref(),
            Some("aqbot:questionnaire:skip_interview")
        );
    }

    #[test]
    fn canonical_tool_output_wins_if_it_arrives_before_the_interaction_closes() {
        let mut tools = HashMap::from([(
            "tool-1".into(),
            PersistedAcpToolCall {
                tool_call_id: "tool-1".into(),
                tool_name: "ask_user_question".into(),
                status: "success".into(),
                input: None,
                output: Some("Agent-recorded result".into()),
                approval_status: None,
                approval_option_id: None,
                approval_option_kind: None,
                approval_label: None,
                sequence: 0,
            },
        )]);
        let mut next_sequence = 1;

        record_interaction_outcome(
            &mut tools,
            &mut next_sequence,
            "tool-1",
            AcpInteractionKind::PlanReview,
            AcpInteractionOutcome::Selected,
            Some("skip_interview"),
            None,
            Some(""),
        );

        assert_eq!(
            tools["tool-1"].output.as_deref(),
            Some("Agent-recorded result")
        );
    }

    #[test]
    fn turn_terminal_state_closes_only_unfinished_tool_calls() {
        let tool = |id: &str, status: &str| PersistedAcpToolCall {
            tool_call_id: id.into(),
            tool_name: "execute".into(),
            status: status.into(),
            input: None,
            output: None,
            approval_status: None,
            approval_option_id: None,
            approval_option_kind: None,
            approval_label: None,
            sequence: 0,
        };
        let mut tools = HashMap::from([
            ("queued".into(), tool("queued", "queued")),
            ("running".into(), tool("running", "in_progress")),
            ("success".into(), tool("success", "completed")),
            ("failed".into(), tool("failed", "error")),
        ]);

        finalize_unfinished_tool_calls(&mut tools, "cancelled");

        assert_eq!(tools["queued"].status, "cancelled");
        assert_eq!(tools["running"].status, "cancelled");
        assert_eq!(tools["success"].status, "completed");
        assert_eq!(tools["failed"].status, "error");
    }

    #[test]
    fn tool_marker_truncates_unicode_on_character_boundaries() {
        let title = format!("{}🙂🙂", "中".repeat(159));
        let marker = build_acp_tool_call_marker(
            "tool-unicode",
            "assistant-unicode",
            &Some(title.clone()),
            &Some("execute".into()),
            &serde_json::Value::Null,
        );
        let expected = format!("{}…", title.chars().take(160).collect::<String>());

        assert!(marker.contains(&expected));
        assert!(!marker.contains(&title));
        assert!(marker.contains("message=\"assistant-unicode\""));
    }

    #[test]
    fn plan_marker_embeds_request_and_message_ids() {
        let marker = build_acp_plan_marker(
            "plan-1",
            "assistant-1",
            &Some("Plan review".into()),
            "## Plan\n1. Inspect\n2. Ship",
            "pending",
        );
        assert!(marker.contains(
            "<acp-plan data-aqbot=\"1\" id=\"plan-1\" message=\"assistant-1\" status=\"pending\" title=\"Plan review\">"
        ));
        assert!(marker.contains("## Plan"));
        assert!(marker.contains("1. Inspect"));
        assert!(marker.contains("</acp-plan>"));
    }

    #[test]
    fn plan_marker_escapes_body_so_nested_tags_do_not_close_early() {
        let marker = build_acp_plan_marker(
            "plan-2",
            "assistant-2",
            &None,
            "use </acp-plan> carefully & <br>",
            "approved",
        );
        assert!(marker.contains("status=\"approved\""));
        assert!(marker.contains("&lt;/acp-plan&gt;"));
        assert!(marker.contains("&amp;"));
        assert!(marker.contains("&lt;br&gt;"));
        assert!(marker.ends_with("</acp-plan>\n\n") || marker.contains("</acp-plan>\n\n"));
    }

    #[test]
    fn patch_plan_marker_status_rewrites_existing_marker() {
        let mut acc = build_acp_plan_marker(
            "plan-1",
            "assistant-1",
            &Some("Plan".into()),
            "body",
            "pending",
        );
        assert!(patch_acp_plan_marker_status(&mut acc, "plan-1", "approved"));
        assert!(acc.contains("status=\"approved\""));
        assert!(!acc.contains("status=\"pending\""));
    }

    #[test]
    fn extract_plan_content_prefers_plan_content_field() {
        let raw = serde_json::json!({
            "title": "short",
            "planContent": "## Full plan body",
            "description": "fallback"
        });
        assert_eq!(
            extract_plan_content_from_raw(&raw).as_deref(),
            Some("## Full plan body")
        );
    }
}

fn json_detail(value: Option<&serde_json::Value>) -> Option<String> {
    let value = value?;
    if value.is_null() {
        return None;
    }
    Some(match value {
        serde_json::Value::String(text) => text.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
    })
}

fn tool_input_detail(raw: &serde_json::Value) -> Option<String> {
    json_detail(
        raw.get("rawInput")
            .or_else(|| raw.get("raw_input"))
            .or_else(|| raw.get("input"))
            .or_else(|| raw.get("locations")),
    )
}

fn tool_output_detail(raw: &serde_json::Value) -> Option<String> {
    json_detail(
        raw.get("rawOutput")
            .or_else(|| raw.get("raw_output"))
            .or_else(|| raw.get("output"))
            .or_else(|| raw.get("content")),
    )
}

fn runtime() -> Arc<AcpRuntime> {
    ACP_RUNTIME
        .get_or_init(|| Arc::new(AcpRuntime::new()))
        .clone()
}

pub(crate) fn config_lock() -> &'static Mutex<()> {
    ACP_CONFIG_LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) fn note_launch_config_changed() {
    ACP_LAUNCH_CONFIG_GENERATION.fetch_add(1, AtomicOrdering::SeqCst);
}

fn agent_launch_changed(before: Option<&ConfiguredAgent>, after: Option<&ConfiguredAgent>) -> bool {
    match (before, after) {
        (Some(before), Some(after)) => {
            before.enabled != after.enabled
                || before.command != after.command
                || before.args != after.args
                || before.env != after.env
        }
        (None, None) => false,
        _ => true,
    }
}

fn note_agent_launch_change(
    before: Option<&ConfiguredAgent>,
    after: Option<&ConfiguredAgent>,
) -> bool {
    let changed = agent_launch_changed(before, after);
    if changed {
        note_launch_config_changed();
    }
    changed
}

fn general_launch_changed(before: &AcpGeneralConfig, after: &AcpGeneralConfig) -> bool {
    before.idle_timeout_secs != after.idle_timeout_secs
        || before.max_concurrent_processes != after.max_concurrent_processes
        || before.permission_default != after.permission_default
}

fn process_proxy_settings(settings: &AppSettings) -> ProcessProxySettings {
    ProcessProxySettings {
        proxy_type: settings.proxy_type.clone(),
        address: settings.proxy_address.clone(),
        port: settings.proxy_port,
    }
}

async fn load_process_proxy_settings(state: &AppState) -> Result<ProcessProxySettings, String> {
    let settings = aqbot_core::repo::settings::get_settings(&state.sea_db)
        .await
        .map_err(|error| error.to_string())?;
    Ok(process_proxy_settings(&settings))
}

struct LockedLaunchConfig {
    file: AcpAgentsFile,
    proxy: ProcessProxySettings,
    launch_generation: u64,
    _guard: tokio::sync::MutexGuard<'static, ()>,
}

/// Take one authoritative Agent launch snapshot. Holding the returned guard
/// until a process/session/prompt is accepted prevents older Agent or proxy
/// settings from being committed after a configuration mutation.
async fn load_locked_launch_config(state: &AppState) -> Result<LockedLaunchConfig, String> {
    let guard = config_lock().lock().await;
    let file = load_agents_file().map_err(|error| error.to_string())?;
    let proxy = load_process_proxy_settings(state).await?;
    Ok(LockedLaunchConfig {
        file,
        proxy,
        launch_generation: ACP_LAUNCH_CONFIG_GENERATION.load(AtomicOrdering::SeqCst),
        _guard: guard,
    })
}

fn agent_with_process_proxy(
    agent: ConfiguredAgent,
    proxy: &ProcessProxySettings,
) -> Result<ConfiguredAgent, String> {
    let agent_id = agent.id.clone();
    configured_agent_with_proxy(agent, proxy, resolve_system_proxy)
        .map_err(|error| format!("failed to configure proxy for ACP agent `{agent_id}`: {error}"))
}

pub(crate) fn configured_agent_ids() -> Result<Vec<String>, String> {
    Ok(load_agents_file()
        .map_err(|error| error.to_string())?
        .agents
        .into_iter()
        .map(|agent| agent.id)
        .collect())
}

pub(crate) async fn invalidate_idle_agent_sessions(agent_ids: &[String]) {
    runtime().drop_agent_sessions(agent_ids).await;
}

#[cfg(test)]
mod proxy_settings_tests {
    use super::{
        config_lock, launch_config_generation_is_current, note_agent_launch_change,
        note_launch_config_changed, overlay_enabled_agents_or_cleanup, process_proxy_settings,
        run_after_config_unlock, ACP_LAUNCH_CONFIG_GENERATION,
    };
    use aqbot_acp_client::config::{AcpAgentsFile, ConfiguredAgent};
    use aqbot_acp_client::proxy::ProcessProxySettings;
    use aqbot_core::types::AppSettings;
    use std::collections::HashMap;
    use std::sync::atomic::Ordering as AtomicOrdering;
    use std::time::Duration;
    use tokio::sync::oneshot;

    #[test]
    fn app_settings_map_to_process_proxy_settings_without_losing_system_mode() {
        let settings = AppSettings {
            proxy_type: Some("system".into()),
            proxy_address: Some("127.0.0.1".into()),
            proxy_port: Some(7890),
            ..AppSettings::default()
        };

        let proxy = process_proxy_settings(&settings);

        assert_eq!(proxy.proxy_type.as_deref(), Some("system"));
        assert_eq!(proxy.address.as_deref(), Some("127.0.0.1"));
        assert_eq!(proxy.port, Some(7890));
    }

    #[tokio::test]
    async fn slow_prewarm_work_does_not_block_foreground_launch_config() {
        let guard = config_lock().lock().await;
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let slow_prewarm = tokio::spawn(run_after_config_unlock(guard, async move {
            let _ = started_tx.send(());
            let _ = release_rx.await;
        }));

        started_rx.await.expect("slow prewarm must start");
        let foreground_guard = tokio::time::timeout(Duration::from_secs(1), config_lock().lock())
            .await
            .expect("foreground prepare must not wait for slow prewarm");
        drop(foreground_guard);
        let _ = release_tx.send(());
        slow_prewarm.await.expect("slow prewarm task must finish");
    }

    #[tokio::test]
    async fn launch_generation_rejects_proxy_disable_and_upsert_races() {
        let guard = config_lock().lock().await;
        let generation = ACP_LAUNCH_CONFIG_GENERATION.load(AtomicOrdering::SeqCst);

        // App proxy save.
        note_launch_config_changed();

        let original = ConfiguredAgent {
            id: "test-agent".into(),
            name: "Test Agent".into(),
            enabled: true,
            source: "custom".into(),
            command: "agent-v1".into(),
            args: Vec::new(),
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };
        let mut disabled = original.clone();
        disabled.enabled = false;
        assert!(note_agent_launch_change(Some(&original), Some(&disabled)));

        let mut upserted = original.clone();
        upserted.command = "agent-v2".into();
        assert!(note_agent_launch_change(Some(&original), Some(&upserted)));
        assert_eq!(
            ACP_LAUNCH_CONFIG_GENERATION.load(AtomicOrdering::SeqCst),
            generation + 3
        );
        drop(guard);

        assert!(!launch_config_generation_is_current(generation).await);
        assert!(launch_config_generation_is_current(generation + 3).await);
    }

    #[tokio::test]
    async fn invalid_latest_proxy_cleans_all_idle_agent_ids_before_erroring() {
        let guard = config_lock().lock().await;
        let enabled = ConfiguredAgent {
            id: "enabled-agent".into(),
            name: "Enabled".into(),
            enabled: true,
            source: "custom".into(),
            command: "enabled-agent".into(),
            args: Vec::new(),
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };
        let mut disabled = enabled.clone();
        disabled.id = "disabled-agent".into();
        disabled.name = "Disabled".into();
        disabled.enabled = false;
        let file = AcpAgentsFile {
            agents: vec![enabled, disabled],
            ..AcpAgentsFile::default()
        };
        let proxy = ProcessProxySettings {
            proxy_type: Some("http".into()),
            address: None,
            port: Some(7890),
        };
        let cleaned_ids = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let captured = cleaned_ids.clone();

        let error = overlay_enabled_agents_or_cleanup(&file, &proxy, move |agent_ids| async move {
            *captured.lock().await = agent_ids;
        })
        .await
        .expect_err("invalid proxy must reject prewarm");
        drop(guard);

        assert!(error.contains("proxy address is required"), "{error}");
        assert_eq!(
            *cleaned_ids.lock().await,
            vec!["enabled-agent".to_string(), "disabled-agent".to_string()]
        );
    }
}

// ---------- Registry & config ----------

#[tauri::command]
pub async fn acp_get_registry() -> Result<RegistryFile, String> {
    load_registry().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_refresh_registry(state: State<'_, AppState>) -> Result<RegistryFile, String> {
    let proxy_settings = load_process_proxy_settings(&state).await?;
    let proxy = resolve_proxy_environment(&proxy_settings).map_err(|error| error.to_string())?;
    let registry = refresh_registry_with_proxy(&proxy)
        .await
        .map_err(|e| e.to_string())?;
    let _guard = config_lock().lock().await;
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    let previous_agents = file.agents.clone();
    let synced = sync_configured_registry_agents(&mut file, &registry)
        .map_err(|error| format!("failed to apply refreshed ACP Registry: {error}"))?;
    let changed_agents = file
        .agents
        .iter()
        .filter(|agent| agent.source == "registry")
        .filter(|agent| {
            previous_agents
                .iter()
                .find(|previous| previous.id == agent.id)
                .is_some_and(|previous| {
                    previous.enabled != agent.enabled
                        || previous.command != agent.command
                        || previous.args != agent.args
                        || previous.env != agent.env
                })
        })
        .map(|agent| agent.id.clone())
        .collect::<Vec<_>>();
    if synced > 0 {
        save_agents_file(&file).map_err(|e| e.to_string())?;
    }
    if !changed_agents.is_empty() {
        note_launch_config_changed();
    }
    runtime().drop_agent_sessions(&changed_agents).await;
    Ok(registry)
}

#[tauri::command]
pub async fn acp_get_config() -> Result<AcpAgentsFile, String> {
    let _guard = config_lock().lock().await;
    migrate_agents_file().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_save_general(general: AcpGeneralConfig) -> Result<AcpAgentsFile, String> {
    let _guard = config_lock().lock().await;
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    let launch_changed = general_launch_changed(&file.general, &general);
    let agent_ids = launch_changed.then(|| {
        file.agents
            .iter()
            .map(|agent| agent.id.clone())
            .collect::<Vec<_>>()
    });
    file.general = general;
    save_agents_file(&file).map_err(|e| e.to_string())?;
    if let Some(agent_ids) = agent_ids {
        note_launch_config_changed();
        runtime().drop_agent_sessions(&agent_ids).await;
    }
    Ok(file)
}

#[tauri::command]
pub async fn acp_add_agent_from_registry(
    agent_id: String,
    enabled: Option<bool>,
) -> Result<AcpAgentsFile, String> {
    let _guard = config_lock().lock().await;
    let registry = load_registry().map_err(|e| e.to_string())?;
    let agent = find_registry_agent(&registry, &agent_id)
        .ok_or_else(|| format!("agent `{agent_id}` not in registry"))?;
    if let Some(reason) = agent.quarantine_reason.as_deref() {
        return Err(format!(
            "agent `{agent_id}` is quarantined by the official ACP Registry: {reason}"
        ));
    }
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    let previous = file
        .agents
        .iter()
        .find(|configured| configured.id == agent_id)
        .cloned();
    upsert_from_registry(&mut file, agent, enabled.unwrap_or(true)).map_err(|e| e.to_string())?;
    save_agents_file(&file).map_err(|e| e.to_string())?;
    let current = file.agents.iter().find(|agent| agent.id == agent_id);
    if note_agent_launch_change(previous.as_ref(), current) {
        runtime()
            .drop_agent_sessions(std::slice::from_ref(&agent_id))
            .await;
    }
    Ok(file)
}

#[tauri::command]
pub async fn acp_upsert_custom_agent(agent: ConfiguredAgent) -> Result<AcpAgentsFile, String> {
    let _guard = config_lock().lock().await;
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    let agent_id = agent.id.clone();
    let previous = file
        .agents
        .iter()
        .find(|configured| configured.id == agent_id)
        .cloned();
    if let Some(existing) = file.agents.iter_mut().find(|a| a.id == agent.id) {
        *existing = agent;
    } else {
        file.agents.push(agent);
    }
    save_agents_file(&file).map_err(|e| e.to_string())?;
    let current = file.agents.iter().find(|agent| agent.id == agent_id);
    if note_agent_launch_change(previous.as_ref(), current) {
        runtime()
            .drop_agent_sessions(std::slice::from_ref(&agent_id))
            .await;
    }
    Ok(file)
}

#[tauri::command]
pub async fn acp_set_agent_enabled(
    agent_id: String,
    enabled: bool,
) -> Result<AcpAgentsFile, String> {
    let _guard = config_lock().lock().await;
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    let previous = file
        .agents
        .iter()
        .find(|agent| agent.id == agent_id)
        .cloned();
    if enabled
        && file
            .agents
            .iter()
            .find(|agent| agent.id == agent_id)
            .is_some_and(|agent| {
                agent.source == "registry"
                    && aqbot_acp_client::registry::official_quarantine_reason(&agent.id).is_some()
            })
    {
        return Err(format!(
            "agent `{agent_id}` is quarantined by the official ACP Registry"
        ));
    }
    if !set_agent_enabled(&mut file, &agent_id, enabled) {
        return Err(format!("agent `{agent_id}` not configured"));
    }
    save_agents_file(&file).map_err(|e| e.to_string())?;
    let current = file.agents.iter().find(|agent| agent.id == agent_id);
    if note_agent_launch_change(previous.as_ref(), current) {
        runtime()
            .drop_agent_sessions(std::slice::from_ref(&agent_id))
            .await;
    }
    Ok(file)
}

#[tauri::command]
pub async fn acp_reorder_agents(agent_ids: Vec<String>) -> Result<AcpAgentsFile, String> {
    let _guard = config_lock().lock().await;
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    reorder_agents(&mut file, &agent_ids);
    save_agents_file(&file).map_err(|e| e.to_string())?;
    Ok(file)
}

#[tauri::command]
pub async fn acp_remove_agent(agent_id: String) -> Result<AcpAgentsFile, String> {
    let _guard = config_lock().lock().await;
    let mut file = load_agents_file().map_err(|e| e.to_string())?;
    let previous = file
        .agents
        .iter()
        .find(|agent| agent.id == agent_id)
        .cloned();
    if !remove_agent(&mut file, &agent_id) {
        return Err(format!("agent `{agent_id}` not configured"));
    }
    save_agents_file(&file).map_err(|e| e.to_string())?;
    if note_agent_launch_change(previous.as_ref(), None) {
        runtime()
            .drop_agent_sessions(std::slice::from_ref(&agent_id))
            .await;
    }
    Ok(file)
}

#[tauri::command]
pub async fn acp_list_enabled_agents() -> Result<Vec<ConfiguredAgent>, String> {
    let file = load_agents_file().map_err(|e| e.to_string())?;
    Ok(enabled_agents(&file).into_iter().cloned().collect())
}

#[tauri::command]
pub async fn acp_probe_agent(
    state: State<'_, AppState>,
    agent_id: String,
) -> Result<AgentProbeResult, String> {
    let file = load_agents_file().map_err(|e| e.to_string())?;
    let agent = file
        .agents
        .iter()
        .find(|a| a.id == agent_id)
        .cloned()
        .ok_or_else(|| format!("agent `{agent_id}` not configured"))?;
    let proxy = load_process_proxy_settings(&state).await?;
    let agent = agent_with_process_proxy(agent, &proxy)?;
    Ok(probe_agent(&agent))
}

#[tauri::command]
pub async fn acp_probe_all(state: State<'_, AppState>) -> Result<Vec<AgentProbeResult>, String> {
    let file = load_agents_file().map_err(|e| e.to_string())?;
    let proxy = load_process_proxy_settings(&state).await?;
    let agents = file
        .agents
        .into_iter()
        .map(|agent| agent_with_process_proxy(agent, &proxy))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(agents.iter().map(probe_agent).collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLaunchView {
    pub agent_id: String,
    pub command: String,
    pub args: Vec<String>,
    pub kind: String,
}

#[tauri::command]
pub async fn acp_resolve_launch(agent_id: String) -> Result<Option<ResolvedLaunchView>, String> {
    let registry = load_registry().map_err(|e| e.to_string())?;
    let Some(agent) = find_registry_agent(&registry, &agent_id) else {
        return Ok(None);
    };
    Ok(resolve_launch(agent).map(|l| ResolvedLaunchView {
        agent_id,
        command: l.command,
        args: l.args,
        kind: l.kind,
    }))
}

// ---------- Projects / threads / messages ----------

#[tauri::command]
pub async fn acp_list_projects(
    state: State<'_, AppState>,
) -> Result<Vec<aqbot_core::entity::acp_projects::Model>, String> {
    acp_repo::list_projects(&state.sea_db)
        .await
        .map_err(|e| e.to_string())
}

/// Reorder projects like conversation categories (drag-and-drop sort).
#[tauri::command]
pub async fn acp_reorder_projects(
    state: State<'_, AppState>,
    project_ids: Vec<String>,
) -> Result<(), String> {
    acp_repo::reorder_projects(&state.sea_db, &project_ids)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_list_all_threads(
    state: State<'_, AppState>,
) -> Result<Vec<aqbot_core::entity::acp_threads::Model>, String> {
    acp_repo::list_all_threads(&state.sea_db)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_create_project(
    state: State<'_, AppState>,
    name: String,
    root_path: String,
) -> Result<aqbot_core::entity::acp_projects::Model, String> {
    let path = PathBuf::from(&root_path);
    if !path.is_dir() {
        return Err(format!("path is not a directory: {root_path}"));
    }
    acp_repo::create_project(&state.sea_db, &name, &root_path)
        .await
        .map_err(|e| e.to_string())
}

/// Reserve one hidden Recent workspace for the composer before its first prompt.
/// Recent projects are only listed in the sidebar after they own a thread, so
/// this gives ACP a real cwd/session without creating an empty conversation.
#[tauri::command]
pub async fn acp_ensure_recent_draft(
    state: State<'_, AppState>,
) -> Result<aqbot_core::entity::acp_projects::Model, String> {
    let _guard = ACP_RECENT_DRAFT_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .await;
    if let Some(project) = reusable_recent_draft(&state.sea_db).await? {
        std::fs::create_dir_all(&project.root_path)
            .map_err(|error| format!("failed to restore ACP draft workspace: {error}"))?;
        return Ok(project);
    }

    let mut settings = aqbot_core::repo::settings::get_settings(&state.sea_db)
        .await
        .map_err(|error| error.to_string())?;
    settings.agent_workspace_root =
        aqbot_core::path_vars::decode_path_opt(&settings.agent_workspace_root);
    create_recent_workspace_project(&state, &settings, "New conversation", true).await
}

#[tauri::command]
pub async fn acp_delete_project(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<(), String> {
    let runtime = runtime();
    delete_project_with_runtime(&state.sea_db, &runtime, &project_id).await
}

async fn delete_project_with_runtime(
    db: &sea_orm::DatabaseConnection,
    runtime: &AcpRuntime,
    project_id: &str,
) -> Result<(), String> {
    let thread_ids = acp_repo::list_threads_for_project(db, project_id)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|thread| thread.id)
        .collect::<Vec<_>>();
    for thread_id in &thread_ids {
        runtime
            .close_session(thread_id)
            .await
            .map_err(|error| format!("failed to close ACP thread `{thread_id}`: {error}"))?;
    }
    acp_repo::delete_project(db, project_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_update_project(
    state: State<'_, AppState>,
    project_id: String,
    name: Option<String>,
    root_path: Option<String>,
) -> Result<aqbot_core::entity::acp_projects::Model, String> {
    if let Some(ref path) = root_path {
        let pb = PathBuf::from(path);
        if !pb.is_dir() {
            return Err(format!("path is not a directory: {path}"));
        }
    }
    acp_repo::update_project(
        &state.sea_db,
        &project_id,
        name.as_deref(),
        root_path.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "project not found".to_string())
}

#[tauri::command]
pub async fn acp_list_threads(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<aqbot_core::entity::acp_threads::Model>, String> {
    let _ = acp_repo::touch_project(&state.sea_db, &project_id).await;
    acp_repo::list_threads_for_project(&state.sea_db, &project_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_create_thread(
    state: State<'_, AppState>,
    project_id: String,
    agent_id: String,
    title: Option<String>,
) -> Result<aqbot_core::entity::acp_threads::Model, String> {
    let file = load_agents_file().map_err(|e| e.to_string())?;
    if !file
        .agents
        .iter()
        .any(|agent| agent.id == agent_id && is_agent_enabled(agent))
    {
        return Err(format!("agent `{agent_id}` is not enabled"));
    }
    let title = title
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "New conversation".into());
    let project = acp_repo::get_project(&state.sea_db, &project_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    let runtime = runtime();
    let draft_key = draft_session_key(&project_id, &agent_id);
    let (thread, draft_metadata_persisted) = match project.kind.as_str() {
        "recent_draft" => {
            let _guard = ACP_RECENT_DRAFT_LOCK
                .get_or_init(|| Mutex::new(()))
                .lock()
                .await;
            let snapshot = runtime
                .session_snapshot(&draft_key)
                .await
                .map_err(|error| format!("failed to inspect ACP Recent draft: {error}"))?;
            let mode_id = snapshot.as_ref().and_then(persisted_mode_id);
            let thread = acp_repo::claim_recent_draft_thread(
                &state.sea_db,
                &project_id,
                &agent_id,
                &title,
                snapshot.as_ref().map(|value| value.session_id.as_str()),
                mode_id.as_deref(),
            )
            .await
            .map_err(|error| error.to_string())?;
            (thread, snapshot.is_some())
        }
        "project" => (
            acp_repo::create_thread(&state.sea_db, &project_id, &agent_id, &title)
                .await
                .map_err(|error| error.to_string())?,
            false,
        ),
        _ => {
            return Err(format!(
                "ACP project `{project_id}` cannot accept another thread"
            ));
        }
    };
    let adopted = runtime.adopt_session(&draft_key, &thread.id).await;
    if !adopted || draft_metadata_persisted {
        return Ok(thread);
    }

    let snapshot = runtime
        .session_snapshot(&thread.id)
        .await
        .map_err(|error| format!("failed to inspect adopted ACP draft: {error}"))?
        .ok_or_else(|| "adopted ACP draft disappeared before persistence".to_string())?;
    persist_live_thread_snapshot(&state.sea_db, &thread.id, &snapshot, None).await?;
    acp_repo::get_thread(&state.sea_db, &thread.id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "newly created ACP thread disappeared".to_string())
}

#[tauri::command]
pub async fn acp_create_recent_thread(
    state: State<'_, AppState>,
    agent_id: String,
    title: Option<String>,
) -> Result<AcpRecentThreadReceipt, String> {
    let file = load_agents_file().map_err(|e| e.to_string())?;
    if !file
        .agents
        .iter()
        .any(|agent| agent.id == agent_id && is_agent_enabled(agent))
    {
        return Err(format!("agent `{agent_id}` is not enabled"));
    }
    let _guard = ACP_RECENT_DRAFT_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .await;

    let mut settings = aqbot_core::repo::settings::get_settings(&state.sea_db)
        .await
        .map_err(|error| error.to_string())?;
    settings.agent_workspace_root =
        aqbot_core::path_vars::decode_path_opt(&settings.agent_workspace_root);

    let title = title
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "New conversation".into());
    let project = create_recent_workspace_project(&state, &settings, &title, false).await?;
    let workspace_dir = PathBuf::from(&project.root_path);

    match acp_repo::create_thread(&state.sea_db, &project.id, &agent_id, &title).await {
        Ok(thread) => Ok(AcpRecentThreadReceipt { project, thread }),
        Err(error) => {
            if let Err(rollback) = acp_repo::delete_project(&state.sea_db, &project.id).await {
                return Err(format!(
                    "{error}; failed to roll back ACP Recent project: {rollback}"
                ));
            }
            std::fs::remove_dir(&workspace_dir).map_err(|cleanup| {
                format!("{error}; failed to remove empty ACP workspace: {cleanup}")
            })?;
            Err(error.to_string())
        }
    }
}

#[tauri::command]
pub async fn acp_delete_thread(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<(), String> {
    let runtime = runtime();
    delete_thread_with_runtime(&state.sea_db, &runtime, &thread_id).await
}

async fn delete_thread_with_runtime(
    db: &sea_orm::DatabaseConnection,
    runtime: &AcpRuntime,
    thread_id: &str,
) -> Result<(), String> {
    let project = match acp_repo::get_thread(db, thread_id)
        .await
        .map_err(|error| error.to_string())?
    {
        Some(thread) => acp_repo::get_project(db, &thread.project_id)
            .await
            .map_err(|error| error.to_string())?,
        None => None,
    };
    runtime
        .close_session(thread_id)
        .await
        .map_err(|error| format!("failed to close ACP thread `{thread_id}`: {error}"))?;
    acp_repo::delete_thread(db, thread_id)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(project) = project.filter(|project| project.kind == "recent") {
        let remaining = acp_repo::list_threads_for_project(db, &project.id)
            .await
            .map_err(|error| error.to_string())?;
        if remaining.is_empty() {
            acp_repo::delete_project(db, &project.id)
                .await
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod session_delete_tests {
    use super::*;

    #[tokio::test]
    async fn close_failure_preserves_thread_and_project_records_and_live_session() {
        const AGENT: &str = r#"
import json
import sys

def respond(request_id, result):
    print(json.dumps({"jsonrpc": "2.0", "id": request_id, "result": result}), flush=True)

for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    if method == "initialize":
        respond(message["id"], {
            "protocolVersion": 1,
            "agentCapabilities": {"sessionCapabilities": {"close": {}}}
        })
    elif method == "session/new":
        respond(message["id"], {"sessionId": "delete-failure-session"})
    elif method == "session/close":
        print(json.dumps({
            "jsonrpc": "2.0",
            "id": message["id"],
            "error": {"code": -32000, "message": "forced close rejection"}
        }), flush=True)
"#;
        let db = aqbot_core::db::create_test_pool().await.unwrap().conn;
        let project = acp_repo::create_project(&db, "Project", "/tmp/project")
            .await
            .unwrap();
        let thread = acp_repo::create_thread(&db, &project.id, "failing-close", "Thread")
            .await
            .unwrap();
        let agent = ConfiguredAgent {
            id: "failing-close".into(),
            name: "Failing close".into(),
            enabled: true,
            source: "custom".into(),
            command: "python3".into(),
            args: vec!["-u".into(), "-c".into(), AGENT.into()],
            env: HashMap::new(),
            icon: None,
            sort: 0,
        };
        let runtime = AcpRuntime::new();
        runtime
            .prepare(
                &thread.id,
                &agent,
                std::env::current_dir().expect("current directory"),
                None,
                false,
                RuntimeLimits::new(60, 1),
                mpsc::unbounded_channel().0,
            )
            .await
            .expect("prepare deletable thread");

        let error = delete_thread_with_runtime(&db, &runtime, &thread.id)
            .await
            .expect_err("close rejection must abort deletion");

        assert!(error.contains("forced close rejection"), "{error}");
        assert!(acp_repo::get_thread(&db, &thread.id)
            .await
            .unwrap()
            .is_some());
        assert!(runtime.has_live_session(&thread.id).await);

        let project_error = delete_project_with_runtime(&db, &runtime, &project.id)
            .await
            .expect_err("close rejection must abort project deletion");
        assert!(
            project_error.contains("forced close rejection"),
            "{project_error}"
        );
        assert!(acp_repo::get_project(&db, &project.id)
            .await
            .unwrap()
            .is_some());
        assert!(acp_repo::get_thread(&db, &thread.id)
            .await
            .unwrap()
            .is_some());
        assert!(runtime.has_live_session(&thread.id).await);
    }
}

#[tauri::command]
pub async fn acp_rename_thread(
    state: State<'_, AppState>,
    thread_id: String,
    title: String,
) -> Result<aqbot_core::entity::acp_threads::Model, String> {
    acp_repo::update_thread_title(&state.sea_db, &thread_id, &title)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "thread not found".to_string())
}

#[tauri::command]
pub async fn acp_toggle_thread_pin(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<aqbot_core::entity::acp_threads::Model, String> {
    acp_repo::toggle_thread_pin(&state.sea_db, &thread_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "thread not found".to_string())
}

#[tauri::command]
pub async fn acp_reorder_threads(
    state: State<'_, AppState>,
    project_id: String,
    thread_ids: Vec<String>,
) -> Result<(), String> {
    acp_repo::reorder_threads(&state.sea_db, &project_id, &thread_ids)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_duplicate_thread(
    state: State<'_, AppState>,
    thread_id: String,
    title_suffix: Option<String>,
) -> Result<aqbot_core::entity::acp_threads::Model, String> {
    let suffix = title_suffix.unwrap_or_else(|| " (copy)".into());
    acp_repo::duplicate_thread(&state.sea_db, &thread_id, &suffix)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "thread not found".to_string())
}

#[tauri::command]
pub async fn acp_list_messages(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<Vec<acp_repo::AcpMessageView>, String> {
    if !runtime().has_live_session(&thread_id).await {
        acp_repo::interrupt_streaming_messages(
            &state.sea_db,
            &thread_id,
            "The previous Agent turn was interrupted",
        )
        .await
        .map_err(|error| error.to_string())?;
    }
    acp_repo::list_messages(&state.sea_db, &thread_id)
        .await
        .map_err(|e| e.to_string())
}

// ---------- Prompt / permission ----------

fn runtime_limits(config: &AcpAgentsFile) -> RuntimeLimits {
    RuntimeLimits::new(
        config.general.idle_timeout_secs,
        config.general.max_concurrent_processes,
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpPrewarmResult {
    agent_id: String,
    ready: bool,
    started: bool,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpPromptAccepted {
    user_message: acp_repo::AcpMessageView,
    assistant_message: acp_repo::AcpMessageView,
}

struct PreparedPrewarm {
    agents: Vec<ConfiguredAgent>,
    auto_approve: bool,
    limits: RuntimeLimits,
    launch_generation: u64,
}

struct LockedPreparedPrewarm {
    prepared: PreparedPrewarm,
    guard: tokio::sync::MutexGuard<'static, ()>,
}

#[tauri::command]
pub async fn acp_prewarm_enabled_agents(
    state: State<'_, AppState>,
) -> Result<Vec<AcpPrewarmResult>, String> {
    let first = prepare_prewarm(&state).await?;
    let (generation, results) = run_prepared_prewarm(first).await;
    if launch_config_generation_is_current(generation).await {
        return Ok(results);
    }

    // A launch-config save raced the first attempt. `prepare_prewarm` retains only
    // current fingerprints before retrying, so the completed stale warm anchor
    // cannot be reused or reported as ready.
    let retry = prepare_prewarm(&state).await?;
    let (generation, results) = run_prepared_prewarm(retry).await;
    if launch_config_generation_is_current(generation).await {
        return Ok(results);
    }

    // Bound retry work while settings are changing rapidly. One final current
    // retain pass removes this attempt's stale anchors without starting more.
    let current = prepare_prewarm(&state).await?;
    let results = current
        .prepared
        .agents
        .iter()
        .map(|agent| AcpPrewarmResult {
            agent_id: agent.id.clone(),
            ready: false,
            started: false,
            error: Some("Agent launch settings changed during prewarm; retry required".into()),
        })
        .collect();
    drop(current);
    Ok(results)
}

async fn prepare_prewarm(state: &AppState) -> Result<LockedPreparedPrewarm, String> {
    let launch = load_locked_launch_config(state).await?;
    let limits = runtime_limits(&launch.file);
    let auto_approve = matches!(
        launch.file.general.permission_default.as_str(),
        "full_access" | "auto_approve"
    );
    let runtime = runtime();
    let cleanup_runtime = runtime.clone();
    let agents = overlay_enabled_agents_or_cleanup(
        &launch.file,
        &launch.proxy,
        move |agent_ids| async move {
            cleanup_runtime.drop_agent_sessions(&agent_ids).await;
        },
    )
    .await?;
    runtime
        .retain_warm_agents(&agents, limits.max_processes)
        .await;
    let LockedLaunchConfig {
        launch_generation,
        _guard: guard,
        ..
    } = launch;
    Ok(LockedPreparedPrewarm {
        prepared: PreparedPrewarm {
            agents,
            auto_approve,
            limits,
            launch_generation,
        },
        guard,
    })
}

async fn overlay_enabled_agents_or_cleanup<Cleanup, CleanupFuture>(
    file: &AcpAgentsFile,
    proxy: &ProcessProxySettings,
    cleanup: Cleanup,
) -> Result<Vec<ConfiguredAgent>, String>
where
    Cleanup: FnOnce(Vec<String>) -> CleanupFuture,
    CleanupFuture: std::future::Future<Output = ()>,
{
    let agents = enabled_agents(file)
        .into_iter()
        .cloned()
        .map(|agent| agent_with_process_proxy(agent, proxy))
        .collect::<Result<Vec<_>, _>>();
    match agents {
        Ok(agents) => Ok(agents),
        Err(error) => {
            cleanup(file.agents.iter().map(|agent| agent.id.clone()).collect()).await;
            Err(error)
        }
    }
}

async fn run_prepared_prewarm(locked: LockedPreparedPrewarm) -> (u64, Vec<AcpPrewarmResult>) {
    let LockedPreparedPrewarm { prepared, guard } = locked;
    let generation = prepared.launch_generation;
    let runtime = runtime();
    let auto_approve = prepared.auto_approve;
    let limits = prepared.limits;
    let tasks = prepared.agents.into_iter().map(|agent| {
        let runtime = runtime.clone();
        async move {
            match runtime.prewarm_agent(&agent, auto_approve, limits).await {
                Ok(started) => AcpPrewarmResult {
                    agent_id: agent.id,
                    ready: true,
                    started,
                    error: None,
                },
                Err(error) => AcpPrewarmResult {
                    agent_id: agent.id,
                    ready: false,
                    started: false,
                    error: Some(error.to_string()),
                },
            }
        }
    });
    let results = run_after_config_unlock(guard, futures::future::join_all(tasks)).await;
    (generation, results)
}

async fn run_after_config_unlock<T>(
    guard: tokio::sync::MutexGuard<'static, ()>,
    operation: impl std::future::Future<Output = T>,
) -> T {
    drop(guard);
    operation.await
}

async fn launch_config_generation_is_current(generation: u64) -> bool {
    let _guard = config_lock().lock().await;
    generation == ACP_LAUNCH_CONFIG_GENERATION.load(AtomicOrdering::SeqCst)
}

fn draft_session_key(project_id: &str, agent_id: &str) -> String {
    format!("draft:{project_id}:{agent_id}")
}

fn is_draft_session_key(session_key: &str) -> bool {
    session_key.starts_with("draft:")
}

async fn persist_live_thread_snapshot(
    db: &sea_orm::DatabaseConnection,
    thread_id: &str,
    snapshot: &AcpSessionSnapshot,
    fallback_mode_id: Option<&str>,
) -> Result<(), String> {
    let mode_id = persisted_mode_id(snapshot).or_else(|| fallback_mode_id.map(str::to_string));
    let persisted = acp_repo::persist_prepared_thread_session(
        db,
        thread_id,
        &snapshot.session_id,
        mode_id.as_deref(),
    )
    .await
    .map_err(|error| error.to_string())?;
    if persisted {
        return Ok(());
    }
    runtime().drop_session(thread_id).await;
    Err(format!(
        "ACP thread `{thread_id}` was deleted while its session was being prepared"
    ))
}

async fn schedule_capability_refresh(
    app: AppHandle,
    db: sea_orm::DatabaseConnection,
    session_key: String,
) {
    let runtime = runtime();
    let Some(handle) = runtime.capability_discovery_handle(&session_key).await else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        match handle.wait().await {
            Ok(Some((current_key, snapshot))) => {
                if !is_draft_session_key(&current_key) {
                    if let Err(error) =
                        persist_live_thread_snapshot(&db, &current_key, &snapshot, None).await
                    {
                        tracing::warn!(%error, thread_id = %current_key, "discarding late ACP capability discovery");
                        return;
                    }
                }
                if let Err(error) = app.emit(
                    "acp-session-state",
                    serde_json::json!({
                        "threadId": current_key,
                        "snapshot": snapshot,
                    }),
                ) {
                    tracing::warn!(%error, "failed to emit discovered ACP capabilities");
                }
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, session_key, "ACP capability discovery refresh failed")
            }
        }
    });
}

#[cfg(test)]
mod draft_session_key_tests {
    use super::is_draft_session_key;

    #[test]
    fn only_reserved_draft_keys_skip_thread_persistence() {
        assert!(is_draft_session_key("draft:project-1:grok-build"));
        assert!(!is_draft_session_key(
            "9ca91146-52cb-44e6-a8cb-ae6df974237f"
        ));
    }
}

fn apply_launch_selection(
    mut agent: ConfiguredAgent,
    model_id: Option<&str>,
    reasoning_effort: Option<&str>,
) -> Result<ConfiguredAgent, String> {
    if let Some(model) = model_id.map(str::trim).filter(|model| !model.is_empty()) {
        agent = configured_agent_with_model(&agent, model).map_err(|error| error.to_string())?;
    }
    if let Some(effort) = reasoning_effort
        .map(str::trim)
        .filter(|effort| !effort.is_empty())
    {
        agent = configured_agent_with_reasoning_effort(&agent, effort)
            .map_err(|error| error.to_string())?;
    }
    Ok(agent)
}

#[tauri::command]
pub async fn acp_prepare_draft(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    agent_id: String,
    model_id: Option<String>,
    reasoning_effort: Option<String>,
) -> Result<AcpSessionSnapshot, String> {
    let project = acp_repo::get_project(&state.sea_db, &project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    let launch = load_locked_launch_config(&state).await?;
    let agent = launch
        .file
        .agents
        .iter()
        .find(|agent| agent.id == agent_id && is_agent_enabled(agent))
        .cloned()
        .ok_or_else(|| format!("agent `{agent_id}` not enabled"))?;
    let agent = apply_launch_selection(agent, model_id.as_deref(), reasoning_effort.as_deref())?;
    let agent = agent_with_process_proxy(agent, &launch.proxy)?;
    let limits = runtime_limits(&launch.file);
    let auto_approve = matches!(
        launch.file.general.permission_default.as_str(),
        "full_access" | "auto_approve"
    );
    let (event_tx, _event_rx) = mpsc::unbounded_channel::<AcpEvent>();
    let session_key = draft_session_key(&project_id, &agent_id);
    let snapshot = runtime()
        .prepare(
            &session_key,
            &agent,
            PathBuf::from(project.root_path),
            None,
            auto_approve,
            limits,
            event_tx,
        )
        .await
        .map_err(|e| e.to_string())?;
    schedule_capability_refresh(app, state.sea_db.clone(), session_key).await;
    drop(launch);
    Ok(snapshot)
}

#[tauri::command]
pub async fn acp_prepare_session(
    app: AppHandle,
    state: State<'_, AppState>,
    thread_id: String,
    model_id: Option<String>,
    reasoning_effort: Option<String>,
) -> Result<AcpSessionSnapshot, String> {
    let thread = acp_repo::get_thread(&state.sea_db, &thread_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "thread not found".to_string())?;
    let project = acp_repo::get_project(&state.sea_db, &thread.project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    let launch = load_locked_launch_config(&state).await?;
    let agent = launch
        .file
        .agents
        .iter()
        .find(|agent| agent.id == thread.agent_id && is_agent_enabled(agent))
        .cloned()
        .ok_or_else(|| format!("agent `{}` not enabled", thread.agent_id))?;
    let agent = apply_launch_selection(agent, model_id.as_deref(), reasoning_effort.as_deref())?;
    let agent = agent_with_process_proxy(agent, &launch.proxy)?;
    let limits = runtime_limits(&launch.file);
    let auto_approve = matches!(
        launch.file.general.permission_default.as_str(),
        "full_access" | "auto_approve"
    );
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AcpEvent>();
    let thread_for_events = thread_id.clone();
    let app_for_events = app.clone();
    let event_task = tauri::async_runtime::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                AcpEvent::SessionState { snapshot } => {
                    let _ = app_for_events.emit(
                        "acp-session-state",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "snapshot": snapshot,
                        }),
                    );
                }
                AcpEvent::Status { message } => {
                    let _ = app_for_events.emit(
                        "acp-status",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "message": message,
                            "preparing": true,
                        }),
                    );
                }
                _ => {}
            }
        }
    });

    let runtime = runtime();
    let mut snapshot = runtime
        .prepare(
            &thread_id,
            &agent,
            PathBuf::from(project.root_path),
            thread.acp_session_id.clone(),
            auto_approve,
            limits,
            event_tx,
        )
        .await
        .map_err(|e| e.to_string())?;
    if let Some(saved_mode) = thread.mode_id.as_deref() {
        match runtime
            .restore_persisted_mode(&thread_id, saved_mode)
            .await
            .map_err(|error| format!("failed to restore ACP mode `{saved_mode}`: {error}"))?
        {
            Some(restored) => snapshot = restored,
            None => {
                tracing::warn!(
                    thread_id = %thread_id,
                    mode_id = %saved_mode,
                    "clearing an ACP session mode that the agent no longer advertises"
                );
            }
        }
    }
    event_task
        .await
        .map_err(|error| format!("ACP prepare event forwarder failed: {error}"))?;
    persist_live_thread_snapshot(&state.sea_db, &thread_id, &snapshot, None).await?;
    schedule_capability_refresh(app, state.sea_db.clone(), thread_id).await;
    drop(launch);
    Ok(snapshot)
}

#[tauri::command]
pub async fn acp_set_config_option(
    state: State<'_, AppState>,
    thread_id: String,
    config_id: String,
    value: serde_json::Value,
) -> Result<AcpSessionSnapshot, String> {
    let snapshot = runtime()
        .set_config_option(&thread_id, &config_id, value)
        .await
        .map_err(|e| e.to_string())?;
    if !is_draft_session_key(&thread_id) {
        persist_live_thread_snapshot(&state.sea_db, &thread_id, &snapshot, None).await?;
    }
    Ok(snapshot)
}

#[tauri::command]
pub async fn acp_set_mode(
    state: State<'_, AppState>,
    thread_id: String,
    mode_id: String,
) -> Result<AcpSessionSnapshot, String> {
    let snapshot = runtime()
        .set_mode(&thread_id, &mode_id)
        .await
        .map_err(|e| e.to_string())?;
    if !is_draft_session_key(&thread_id) {
        persist_live_thread_snapshot(&state.sea_db, &thread_id, &snapshot, Some(&mode_id)).await?;
    }
    Ok(snapshot)
}

#[tauri::command]
pub async fn acp_cancel(state: State<'_, AppState>, thread_id: String) -> Result<bool, String> {
    let cancelled = runtime()
        .cancel(&thread_id)
        .await
        .map_err(|e| e.to_string())?;
    if cancelled {
        return Ok(true);
    }
    let interrupted = acp_repo::interrupt_streaming_messages(
        &state.sea_db,
        &thread_id,
        "The Agent process is no longer running",
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(interrupted > 0)
}

fn attachment_file_uri(
    file_store: &aqbot_core::file_store::FileStore,
    attachment: &Attachment,
) -> Result<String, String> {
    let path = file_store
        .validated_path(&attachment.file_path)
        .map_err(|error| {
            format!(
                "Invalid persisted attachment path for {}: {error}",
                attachment.file_name
            )
        })?;
    reqwest::Url::from_file_path(&path)
        .map(|url| url.to_string())
        .map_err(|_| {
            format!(
                "Could not convert persisted attachment path to a file URI: {}",
                path.display()
            )
        })
}

fn build_prompt_input(
    text: String,
    inputs: &[AttachmentInput],
    persisted: &[Attachment],
) -> Result<AcpPromptInput, String> {
    build_prompt_input_with_store(
        text,
        inputs,
        persisted,
        &aqbot_core::file_store::FileStore::new(),
    )
}

fn build_prompt_input_with_store(
    text: String,
    inputs: &[AttachmentInput],
    persisted: &[Attachment],
    file_store: &aqbot_core::file_store::FileStore,
) -> Result<AcpPromptInput, String> {
    if inputs.len() != persisted.len() {
        return Err(format!(
            "Persisted attachment count mismatch: expected {}, got {}",
            inputs.len(),
            persisted.len()
        ));
    }
    let attachments = inputs
        .iter()
        .zip(persisted)
        .map(|(input, attachment)| {
            let mime_type = aqbot_core::storage_paths::normalize_attachment_mime_type(
                &attachment.file_name,
                &attachment.file_type,
            );
            let is_image = aqbot_core::storage_paths::is_image_mime_type(&mime_type);
            Ok(AcpPromptAttachment {
                file_name: attachment.file_name.clone(),
                mime_type,
                file_size: attachment.file_size,
                data: is_image.then(|| input.data.clone()),
                file_uri: attachment_file_uri(file_store, attachment)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(AcpPromptInput { text, attachments })
}

async fn rollback_prompt_receipt(
    db: &sea_orm::DatabaseConnection,
    thread_id: &str,
    user_message_id: &str,
    assistant_message_id: &str,
    primary: String,
) -> String {
    let ids = vec![
        user_message_id.to_string(),
        assistant_message_id.to_string(),
    ];
    match acp_repo::rollback_prompt_messages(db, thread_id, &ids).await {
        Ok(()) => primary,
        Err(error) => format!("{primary}; ACP prompt rollback failed: {error}"),
    }
}

#[cfg(test)]
mod prompt_input_tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn prompt_input_uses_persisted_file_uris_and_keeps_base64_only_for_images() {
        let root = tempfile::tempdir().unwrap();
        let store = aqbot_core::file_store::FileStore::with_root(root.path().to_path_buf());
        let image_bytes = b"image";
        let file_bytes = b"document";
        let saved_image = store
            .save_file(image_bytes, "my image.png", "image/png")
            .unwrap();
        let saved_file = store
            .save_file(file_bytes, "notes #1.txt", "text/plain")
            .unwrap();
        let inputs = vec![
            AttachmentInput {
                file_name: "my image.png".to_string(),
                file_type: "application/x-custom".to_string(),
                file_size: image_bytes.len() as u64,
                data: base64::engine::general_purpose::STANDARD.encode(image_bytes),
            },
            AttachmentInput {
                file_name: "notes #1.txt".to_string(),
                file_type: "text/plain".to_string(),
                file_size: file_bytes.len() as u64,
                data: base64::engine::general_purpose::STANDARD.encode(file_bytes),
            },
        ];
        let persisted = vec![
            Attachment {
                id: "image-id".to_string(),
                file_type: "application/x-custom".to_string(),
                file_name: "my image.png".to_string(),
                file_path: saved_image.storage_path.clone(),
                file_size: image_bytes.len() as u64,
                data: None,
            },
            Attachment {
                id: "file-id".to_string(),
                file_type: "text/plain".to_string(),
                file_name: "notes #1.txt".to_string(),
                file_path: saved_file.storage_path.clone(),
                file_size: file_bytes.len() as u64,
                data: None,
            },
        ];

        let prompt =
            build_prompt_input_with_store("inspect".to_string(), &inputs, &persisted, &store)
                .unwrap();

        assert_eq!(
            prompt.attachments[0].data.as_deref(),
            Some(inputs[0].data.as_str())
        );
        assert_eq!(prompt.attachments[0].mime_type, "image/png");
        assert!(prompt.attachments[1].data.is_none());
        for (prepared, metadata) in prompt.attachments.iter().zip(&persisted) {
            let url = reqwest::Url::parse(&prepared.file_uri).unwrap();
            assert_eq!(url.scheme(), "file");
            assert_eq!(
                url.to_file_path().unwrap(),
                store.resolve_path(&metadata.file_path)
            );
        }
    }
}

#[tauri::command]
pub async fn acp_prompt(
    app: AppHandle,
    state: State<'_, AppState>,
    thread_id: String,
    prompt: String,
    attachments: Option<Vec<AttachmentInput>>,
    model_id: Option<String>,
    reasoning_effort: Option<String>,
) -> Result<AcpPromptAccepted, String> {
    let attachments = attachments.unwrap_or_default();
    if prompt.trim().is_empty() && attachments.is_empty() {
        return Err("prompt must contain text or attachments".into());
    }
    let thread = acp_repo::get_thread(&state.sea_db, &thread_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "thread not found".to_string())?;

    let project = acp_repo::get_project(&state.sea_db, &thread.project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "project not found".to_string())?;

    let launch = load_locked_launch_config(&state).await?;
    let agent = launch
        .file
        .agents
        .iter()
        .find(|agent| agent.id == thread.agent_id && is_agent_enabled(agent))
        .cloned()
        .ok_or_else(|| format!("agent `{}` not enabled", thread.agent_id))?;
    let agent = apply_launch_selection(agent, model_id.as_deref(), reasoning_effort.as_deref())?;
    let agent = agent_with_process_proxy(agent, &launch.proxy)?;
    let limits = runtime_limits(&launch.file);
    let auto_approve = matches!(
        launch.file.general.permission_default.as_str(),
        "full_access" | "auto_approve"
    );
    let cwd = PathBuf::from(&project.root_path);
    let rt = runtime();

    // Initialization is both a launch preflight and the authoritative source
    // for image capability. Do it before writing files or messages.
    let (prepare_tx, _prepare_rx) = mpsc::unbounded_channel::<AcpEvent>();
    let snapshot = rt
        .prepare(
            &thread_id,
            &agent,
            cwd.clone(),
            thread.acp_session_id.clone(),
            auto_approve,
            limits,
            prepare_tx,
        )
        .await
        .map_err(|error| error.to_string())?;
    if attachments.iter().any(|attachment| {
        aqbot_core::storage_paths::is_image_attachment(&attachment.file_name, &attachment.file_type)
    }) && !snapshot.agent_capabilities.prompt_capabilities.image
    {
        return Err("ACP agent does not advertise image prompt capability".to_string());
    }
    persist_live_thread_snapshot(&state.sea_db, &thread_id, &snapshot, None).await?;

    let (user_message, assistant) =
        acp_repo::create_prompt_messages(&state.sea_db, &thread_id, &prompt, &attachments)
            .await
            .map_err(|error| error.to_string())?;
    let prompt_input = match build_prompt_input(prompt, &attachments, &user_message.attachments) {
        Ok(input) => input,
        Err(error) => {
            return Err(rollback_prompt_receipt(
                &state.sea_db,
                &thread_id,
                &user_message.id,
                &assistant.id,
                error,
            )
            .await)
        }
    };

    let session_id = Some(snapshot.session_id);
    let db = state.sea_db.clone();
    let assistant_id = assistant.id.clone();
    let thread_id_clone = thread_id.clone();

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AcpEvent>();
    let accumulated_text = Arc::new(Mutex::new(String::new()));
    let tool_transcript = Arc::new(Mutex::new(HashMap::<String, PersistedAcpToolCall>::new()));
    let turn_started = std::time::Instant::now();

    // Forward events to frontend.
    // Tool calls are also injected as inline <tool-call> markers into the
    // assistant message text (same pattern as chat agent mode) so they appear
    // in chronological order inside the bubble — not dumped under the thread.
    let app_fwd = app.clone();
    let db_for_events = db.clone();
    let thread_for_events = thread_id.clone();
    let assistant_for_events = assistant_id.clone();
    let acc_for_events = accumulated_text.clone();
    let tools_for_events = tool_transcript.clone();
    let event_task = tauri::async_runtime::spawn(async move {
        let mut thinking_open = false;
        let mut next_tool_sequence = 0_u64;
        while let Some(ev) = event_rx.recv().await {
            match &ev {
                AcpEvent::StreamText { text } => {
                    let display_text = if thinking_open {
                        thinking_open = false;
                        format!("\n</think>\n\n{text}")
                    } else {
                        text.clone()
                    };
                    {
                        let mut acc = acc_for_events.lock().await;
                        acc.push_str(&display_text);
                    }
                    let _ = app_fwd.emit(
                        "acp-stream-text",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "messageId": assistant_for_events,
                            "text": display_text,
                        }),
                    );
                }
                AcpEvent::StreamThinking { thinking } => {
                    let display_text = if thinking_open {
                        thinking.clone()
                    } else {
                        thinking_open = true;
                        format!("<think>\n{thinking}")
                    };
                    {
                        let mut acc = acc_for_events.lock().await;
                        acc.push_str(&display_text);
                    }
                    let _ = app_fwd.emit(
                        "acp-stream-text",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "messageId": assistant_for_events,
                            "text": display_text,
                        }),
                    );
                }
                AcpEvent::PermissionRequest {
                    request_id,
                    interaction_kind,
                    tool_call_id,
                    title,
                    raw,
                    options,
                } => {
                    // Plan reviews are injected as inline markers so the card
                    // stays mid-message (before any later assistant text). Full
                    // plan body is embedded so reloads can rehydrate the card.
                    if matches!(interaction_kind, AcpInteractionKind::PlanReview) {
                        let plan_body = extract_plan_content_from_raw(raw)
                            .or_else(|| {
                                title
                                    .as_ref()
                                    .map(|s| s.trim().to_string())
                                    .filter(|s| !s.is_empty())
                            })
                            .unwrap_or_default();
                        let plan_title = title.clone().or_else(|| {
                            raw.get("title")
                                .and_then(|v| v.as_str())
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(str::to_string)
                        });
                        let marker = if thinking_open {
                            thinking_open = false;
                            format!(
                                "\n</think>\n\n{}",
                                build_acp_plan_marker(
                                    request_id,
                                    &assistant_for_events,
                                    &plan_title,
                                    &plan_body,
                                    "pending",
                                )
                            )
                        } else {
                            build_acp_plan_marker(
                                request_id,
                                &assistant_for_events,
                                &plan_title,
                                &plan_body,
                                "pending",
                            )
                        };
                        let marker_id = format!(
                            "<acp-plan data-aqbot=\"1\" id=\"{}\"",
                            xml_attr_escape(request_id)
                        );
                        let should_emit_marker = {
                            let mut acc = acc_for_events.lock().await;
                            if acc.contains(&marker_id) {
                                false
                            } else {
                                acc.push_str(&marker);
                                true
                            }
                        };
                        if should_emit_marker {
                            let _ = app_fwd.emit(
                                "acp-stream-text",
                                serde_json::json!({
                                    "threadId": thread_for_events,
                                    "messageId": assistant_for_events,
                                    "text": marker,
                                }),
                            );
                        }
                    }
                    let _ = app_fwd.emit(
                        "acp-permission-request",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "messageId": assistant_for_events,
                            "requestId": request_id,
                            "interactionKind": interaction_kind,
                            "toolCallId": tool_call_id,
                            "title": title,
                            "raw": raw,
                            "options": options,
                        }),
                    );
                }
                AcpEvent::InteractionClosed {
                    request_id,
                    interaction_kind,
                    tool_call_id,
                    outcome,
                    selected_option_id,
                    selected_option_kind,
                    selected_option_name,
                } => {
                    if let Some(tool_call_id) = tool_call_id {
                        let mut tools = tools_for_events.lock().await;
                        record_interaction_outcome(
                            &mut tools,
                            &mut next_tool_sequence,
                            tool_call_id,
                            *interaction_kind,
                            *outcome,
                            selected_option_id.as_deref(),
                            selected_option_kind.as_deref(),
                            selected_option_name.as_deref(),
                        );
                    }
                    // Persist final plan-review outcome on the inline marker so
                    // a refresh still shows approved/cancelled/abandoned.
                    if matches!(interaction_kind, AcpInteractionKind::PlanReview) {
                        let status = plan_review_status_from_outcome(
                            *outcome,
                            selected_option_id.as_deref(),
                        );
                        let mut acc = acc_for_events.lock().await;
                        let _ = patch_acp_plan_marker_status(&mut acc, request_id, status);
                    }
                    let _ = app_fwd.emit(
                        "acp-interaction-closed",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "messageId": assistant_for_events,
                            "requestId": request_id,
                            "interactionKind": interaction_kind,
                            "toolCallId": tool_call_id,
                            "reason": outcome,
                            "selectedOptionId": selected_option_id,
                            "selectedOptionKind": selected_option_kind,
                            "selectedOptionName": selected_option_name,
                        }),
                    );
                }
                AcpEvent::ToolCall {
                    tool_call_id,
                    title,
                    kind,
                    status,
                    raw,
                } => {
                    {
                        let mut tools = tools_for_events.lock().await;
                        record_tool_call(
                            &mut tools,
                            &mut next_tool_sequence,
                            tool_call_id,
                            title,
                            kind,
                            status,
                            raw,
                        );
                    }
                    // Chronological inline marker → stream + DB final text
                    let marker = if thinking_open {
                        thinking_open = false;
                        format!(
                            "\n</think>\n\n{}",
                            build_acp_tool_call_marker(
                                tool_call_id,
                                &assistant_for_events,
                                title,
                                kind,
                                raw,
                            )
                        )
                    } else {
                        build_acp_tool_call_marker(
                            tool_call_id,
                            &assistant_for_events,
                            title,
                            kind,
                            raw,
                        )
                    };
                    let id_attr = format!("id=\"{}\"", xml_attr_escape(tool_call_id));
                    let should_emit_marker = {
                        let mut acc = acc_for_events.lock().await;
                        if acc.contains(&id_attr) {
                            false
                        } else {
                            acc.push_str(&marker);
                            true
                        }
                    };
                    if should_emit_marker {
                        let _ = app_fwd.emit(
                            "acp-stream-text",
                            serde_json::json!({
                                "threadId": thread_for_events,
                                "messageId": assistant_for_events,
                                "text": marker,
                            }),
                        );
                    }
                    let _ = app_fwd.emit(
                        "acp-tool-call",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "messageId": assistant_for_events,
                            "toolCallId": tool_call_id,
                            "title": title,
                            "kind": kind,
                            "status": status,
                            "raw": raw,
                        }),
                    );
                }
                AcpEvent::ToolCallUpdate {
                    tool_call_id,
                    status,
                    raw,
                } => {
                    {
                        let mut tools = tools_for_events.lock().await;
                        let sequence = tools.get(tool_call_id).map_or_else(
                            || {
                                let current = next_tool_sequence;
                                next_tool_sequence += 1;
                                current
                            },
                            |tool| tool.sequence,
                        );
                        let tool = tools.entry(tool_call_id.clone()).or_insert_with(|| {
                            PersistedAcpToolCall {
                                tool_call_id: tool_call_id.clone(),
                                tool_name: "tool".into(),
                                status: "running".into(),
                                input: None,
                                output: None,
                                approval_status: None,
                                approval_option_id: None,
                                approval_option_kind: None,
                                approval_label: None,
                                sequence,
                            }
                        });
                        if let Some(status) = status {
                            tool.status = status.clone();
                        }
                        if let Some(input) = tool_input_detail(raw) {
                            tool.input = Some(input);
                        }
                        if let Some(output) = tool_output_detail(raw) {
                            tool.output = Some(output);
                        }
                    }
                    let _ = app_fwd.emit(
                        "acp-tool-call-update",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "messageId": assistant_for_events,
                            "toolCallId": tool_call_id,
                            "status": status,
                            "raw": raw,
                        }),
                    );
                }
                AcpEvent::Plan { raw } => {
                    let _ = app_fwd.emit(
                        "acp-plan",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "messageId": assistant_for_events,
                            "raw": raw,
                        }),
                    );
                }
                AcpEvent::SessionState { snapshot } => {
                    let mode_id = persisted_mode_id(snapshot);
                    if let Err(error) = acp_repo::update_thread_mode(
                        &db_for_events,
                        &thread_for_events,
                        mode_id.as_deref(),
                    )
                    .await
                    {
                        tracing::error!(%error, thread_id = %thread_for_events, "failed to persist ACP session mode update");
                    }
                    let _ = app_fwd.emit(
                        "acp-session-state",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "snapshot": snapshot,
                        }),
                    );
                }
                AcpEvent::Status { message } => {
                    let _ = app_fwd.emit(
                        "acp-status",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "message": message,
                        }),
                    );
                }
                AcpEvent::Error { message } => {
                    let _ = app_fwd.emit(
                        "acp-status",
                        serde_json::json!({
                            "threadId": thread_for_events,
                            "message": message,
                        }),
                    );
                }
                // Runtime emits this only after session/prompt has returned and
                // notification routing has been detached. It is the explicit
                // drain boundary; UI finalization still happens after DB persist.
                AcpEvent::Done { .. } => break,
            }
        }
        if thinking_open {
            let close = "\n</think>\n";
            acc_for_events.lock().await.push_str(close);
            let _ = app_fwd.emit(
                "acp-stream-text",
                serde_json::json!({
                    "threadId": thread_for_events,
                    "messageId": assistant_for_events,
                    "text": close,
                }),
            );
        }
    });

    // `schedule_prompt` is the acceptance boundary: initialization, capability
    // conversion, busy checks, and worker enqueue all complete before the IPC
    // command returns. Any failure here rolls back the just-created receipt.
    let prompt_handle = match rt
        .schedule_prompt(
            &thread_id,
            &agent,
            cwd,
            prompt_input,
            session_id,
            auto_approve,
            limits,
            event_tx,
        )
        .await
    {
        Ok(handle) => handle,
        Err(error) => {
            if let Err(join_error) = event_task.await {
                tracing::warn!(%join_error, thread_id = %thread_id, "ACP event forwarder failed after scheduling rejection");
            }
            return Err(rollback_prompt_receipt(
                &state.sea_db,
                &thread_id,
                &user_message.id,
                &assistant.id,
                error.to_string(),
            )
            .await);
        }
    };
    // The turn is now active, so a concurrent launch-config save preserves it while
    // invalidating only idle sessions and warm anchors for the next turn.
    drop(launch);

    let acc_for_persist = accumulated_text.clone();
    let tools_for_persist = tool_transcript.clone();
    tauri::async_runtime::spawn(async move {
        let result = prompt_handle.wait().await;

        // The channel closes after the worker clears its per-turn sender. Waiting
        // drains every already-delivered notification without an arbitrary sleep.
        if let Err(error) = event_task.await {
            tracing::error!(%error, thread_id = %thread_id_clone, "ACP event forwarder failed");
        }
        let final_text = acc_for_persist.lock().await.clone();
        let duration_ms = turn_started.elapsed().as_millis() as u64;
        let terminal_tool_status = match result.as_ref() {
            Ok(outcome) if outcome.stop_reason.to_ascii_lowercase().contains("cancel") => {
                "cancelled"
            }
            Ok(_) | Err(_) => "error",
        };
        let mut tools = tools_for_persist.lock().await;
        finalize_unfinished_tool_calls(&mut tools, terminal_tool_status);
        let mut tool_calls = tools.values().cloned().collect::<Vec<_>>();
        drop(tools);
        tool_calls.sort_by_key(|tool| tool.sequence);
        let meta = serde_json::json!({
            "duration_ms": duration_ms,
            "toolCalls": tool_calls,
        })
        .to_string();

        match result {
            Ok(outcome) => {
                let persist_result = acp_repo::finalize_prompt(
                    &db,
                    acp_repo::AcpPromptFinalization {
                        thread_id: &thread_id_clone,
                        message_id: &assistant_id,
                        content: &final_text,
                        message_status: "done",
                        meta_json: Some(&meta),
                        acp_session_id: Some(&outcome.session_id),
                        runtime_status: "idle",
                    },
                )
                .await
                .map_err(|error| error.to_string());
                if let Err(error) = persist_result {
                    tracing::error!(%error, thread_id = %thread_id_clone, "failed to persist completed ACP turn");
                    runtime().drop_session(&thread_id_clone).await;
                    if let Err(emit_error) = app.emit(
                        "acp-error",
                        serde_json::json!({
                            "threadId": &thread_id_clone,
                            "messageId": &assistant_id,
                            "message": format!("Failed to persist ACP response: {error}"),
                            "text": final_text,
                            "durationMs": duration_ms,
                        }),
                    ) {
                        tracing::warn!(%emit_error, thread_id = %thread_id_clone, "failed to emit ACP persistence error");
                    }
                    return;
                }
                // Emit AFTER DB write so any subsequent loadMessages sees status=done.
                if let Err(error) = app.emit(
                    "acp-done",
                    serde_json::json!({
                        "threadId": &thread_id_clone,
                        "messageId": &assistant_id,
                        "stopReason": outcome.stop_reason,
                        "sessionId": outcome.session_id,
                        "text": final_text,
                        "durationMs": duration_ms,
                    }),
                ) {
                    tracing::warn!(%error, thread_id = %thread_id_clone, "failed to emit acp-done");
                }
            }
            Err(e) => {
                let err_text = if final_text.is_empty() {
                    format!("Error: {e}")
                } else {
                    format!("{final_text}\n\nError: {e}")
                };
                if let Err(error) = acp_repo::finalize_prompt(
                    &db,
                    acp_repo::AcpPromptFinalization {
                        thread_id: &thread_id_clone,
                        message_id: &assistant_id,
                        content: &err_text,
                        message_status: "error",
                        meta_json: Some(&meta),
                        acp_session_id: None,
                        runtime_status: "error",
                    },
                )
                .await
                {
                    tracing::error!(%error, thread_id = %thread_id_clone, "failed to persist ACP error state");
                }
                if let Err(error) = app.emit(
                    "acp-error",
                    serde_json::json!({
                        "threadId": &thread_id_clone,
                        "messageId": &assistant_id,
                        "message": e.to_string(),
                        "text": err_text,
                        "durationMs": duration_ms,
                    }),
                ) {
                    tracing::warn!(%error, thread_id = %thread_id_clone, "failed to emit acp-error");
                }
            }
        }
    });

    Ok(AcpPromptAccepted {
        user_message,
        assistant_message: assistant,
    })
}

#[tauri::command]
pub async fn acp_respond_permission(
    request_id: String,
    option_id: String,
    feedback: Option<String>,
) -> Result<(), String> {
    if runtime()
        .resolve_permission(&request_id, option_id, feedback)
        .await
    {
        Ok(())
    } else {
        Err("permission request not found or already resolved".into())
    }
}

#[tauri::command]
pub async fn acp_respond_questionnaire(
    request_id: String,
    outcome: AcpQuestionnaireOutcome,
    answers: Vec<AcpQuestionnaireAnswer>,
) -> Result<String, String> {
    runtime()
        .resolve_questionnaire(&request_id, AcpQuestionnaireSubmission { outcome, answers })
        .await
}

/// Debug helper: registry source label for UI.
#[tauri::command]
pub async fn acp_registry_source() -> Result<String, String> {
    let reg = load_registry().map_err(|e| e.to_string())?;
    Ok(match reg.source.unwrap_or(RegistrySource::Builtin) {
        RegistrySource::Builtin => "builtin".into(),
        RegistrySource::Cache => "cache".into(),
        RegistrySource::Live => "live".into(),
    })
}

// ---------- Inline tool-call markers (chat-agent parity) ----------

fn xml_attr_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn xml_text_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Pull plan-review body from the permission/request payload so the marker
/// can be fully reconstructed after a page reload.
fn extract_plan_content_from_raw(raw: &serde_json::Value) -> Option<String> {
    for key in [
        "planContent",
        "plan_content",
        "content",
        "description",
        "plan",
    ] {
        if let Some(text) = raw.get(key).and_then(|v| v.as_str()) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn plan_review_status_from_outcome(
    outcome: AcpInteractionOutcome,
    selected_option_id: Option<&str>,
) -> &'static str {
    let option_status = selected_option_id.and_then(|id| {
        let normalized: String = id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        match normalized.as_str() {
            "approved" | "approve" => Some("approved"),
            "cancelled" | "cancel" => Some("cancelled"),
            "abandoned" | "abandon" => Some("abandoned"),
            _ => None,
        }
    });
    match outcome {
        AcpInteractionOutcome::Expired => "expired",
        AcpInteractionOutcome::Cancelled => option_status.unwrap_or("cancelled"),
        AcpInteractionOutcome::Selected => option_status.unwrap_or("approved"),
    }
}

/// Rewrite `status="..."` on an existing inline plan marker so reloads keep
/// the final review outcome (approved / cancelled / abandoned / expired).
fn patch_acp_plan_marker_status(acc: &mut String, request_id: &str, status: &str) -> bool {
    let id_attr = format!("id=\"{}\"", xml_attr_escape(request_id));
    let Some(id_pos) = acc.find(&id_attr) else {
        return false;
    };
    // Walk back to the opening `<acp-plan` of this marker.
    let prefix = &acc[..id_pos];
    let Some(tag_rel) = prefix.rfind("<acp-plan") else {
        return false;
    };
    let tag_start = tag_rel;
    let Some(tag_end_rel) = acc[tag_start..].find('>') else {
        return false;
    };
    let tag_end = tag_start + tag_end_rel;
    let open_tag = &acc[tag_start..=tag_end];
    if !open_tag.starts_with("<acp-plan") {
        return false;
    }
    let status_attr = format!("status=\"{}\"", xml_attr_escape(status));
    let new_open = if let Some(status_start) = open_tag.find("status=\"") {
        let after = &open_tag[status_start + "status=\"".len()..];
        let Some(quote_end) = after.find('"') else {
            return false;
        };
        format!(
            "{}{}{}",
            &open_tag[..status_start],
            status_attr,
            &open_tag[status_start + "status=\"".len() + quote_end + 1..]
        )
    } else {
        // Insert status just before the closing `>`.
        format!("{} {}>", &open_tag[..open_tag.len() - 1], status_attr)
    };
    acc.replace_range(tag_start..=tag_end, &new_open);
    true
}

/// Build an inline `<acp-plan>` marker so plan reviews render mid-conversation
/// in chronological order (before any later assistant text in the same turn).
///
/// The **body holds the full plan markdown** so the card can be reconstructed
/// after a page refresh without relying on in-memory store state.
fn build_acp_plan_marker(
    request_id: &str,
    message_id: &str,
    title: &Option<String>,
    content: &str,
    status: &str,
) -> String {
    let label = title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("plan");
    let body = if content.trim().is_empty() {
        label
    } else {
        content
    };
    format!(
        "\n\n<acp-plan data-aqbot=\"1\" id=\"{}\" message=\"{}\" status=\"{}\" title=\"{}\">{}</acp-plan>\n\n",
        xml_attr_escape(request_id),
        xml_attr_escape(message_id),
        xml_attr_escape(status),
        xml_attr_escape(label),
        xml_text_escape(body),
    )
}

/// Build an inline `<tool-call>` marker so tools render mid-conversation
/// in call order (same contract as chat agent mode).
fn build_acp_tool_call_marker(
    tool_call_id: &str,
    message_id: &str,
    title: &Option<String>,
    kind: &Option<String>,
    raw: &serde_json::Value,
) -> String {
    // Prefer short kind as the chip name; fall back to title / "tool"
    let name = kind
        .as_deref()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            title
                .as_deref()
                .map(|t| t.split_whitespace().next().unwrap_or(t))
                .filter(|s| !s.is_empty() && s.len() <= 32)
        })
        .unwrap_or("tool");

    let mut summary = title.clone().unwrap_or_default();
    if summary.is_empty() {
        // rawInput.command / path / filePath etc.
        let input = raw
            .get("rawInput")
            .or_else(|| raw.get("raw_input"))
            .or_else(|| raw.get("input"))
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        if let Some(obj) = input.as_object() {
            for key in [
                "command",
                "path",
                "filePath",
                "file_path",
                "pattern",
                "query",
            ] {
                if let Some(v) = obj.get(key).and_then(|x| x.as_str()) {
                    summary = v.to_string();
                    break;
                }
            }
        }
        if summary.is_empty() {
            if let Some(locs) = raw.get("locations").and_then(|v| v.as_array()) {
                if let Some(path) = locs
                    .first()
                    .and_then(|l| l.get("path").or_else(|| l.get("uri")))
                    .and_then(|v| v.as_str())
                {
                    summary = path.to_string();
                }
            }
        }
    }

    // Keep summary readable in the chip
    if summary.chars().count() > 160 {
        summary = format!("{}…", summary.chars().take(160).collect::<String>());
    }
    // Collapse newlines for attr-like chip text
    summary = summary.replace('\n', " ").replace('\r', " ");

    format!(
        "\n\n<tool-call data-aqbot=\"1\" id=\"{}\" message=\"{}\" name=\"{}\">{}</tool-call>\n\n",
        xml_attr_escape(tool_call_id),
        xml_attr_escape(message_id),
        xml_attr_escape(name),
        xml_text_escape(&summary),
    )
}

// ---------- Git (project working tree) ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpGitInfo {
    pub branch: Option<String>,
    pub branches: Vec<String>,
    pub is_repo: bool,
}

fn git_output(cwd: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("git failed: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("git {} failed", args.join(" "))
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn checkout_local_branch(cwd: &std::path::Path, branch: &str) -> Result<(), String> {
    if branch.trim().is_empty() {
        return Err("branch name is empty".into());
    }
    if branch != branch.trim() {
        return Err("branch name must match a local branch exactly".into());
    }
    if branch.starts_with('-') {
        return Err("branch name must not start with '-'".into());
    }

    let local_ref = format!("refs/heads/{branch}");
    git_output(cwd, &["show-ref", "--verify", "--quiet", &local_ref])
        .map_err(|error| format!("local branch `{branch}` is not available: {error}"))?;

    git_output(cwd, &["switch", "--", branch])?;
    Ok(())
}

#[cfg(test)]
mod git_checkout_tests {
    use super::*;

    fn run_git(cwd: &std::path::Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("run git command");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn initialized_repository() -> tempfile::TempDir {
        let repository = tempfile::tempdir().expect("create temporary repository");
        let cwd = repository.path();
        run_git(cwd, &["init"]);
        run_git(cwd, &["config", "user.name", "AQBot Test"]);
        run_git(cwd, &["config", "user.email", "aqbot@example.invalid"]);
        std::fs::write(cwd.join("tracked.txt"), "committed\n").expect("write tracked file");
        run_git(cwd, &["add", "tracked.txt"]);
        run_git(cwd, &["commit", "-m", "initial"]);
        repository
    }

    #[test]
    fn option_like_branch_is_rejected_without_discarding_dirty_changes() {
        let repository = initialized_repository();
        let cwd = repository.path();
        let tracked = cwd.join("tracked.txt");
        std::fs::write(&tracked, "dirty\n").expect("make tracked file dirty");

        let result = checkout_local_branch(cwd, "-f");
        let content = std::fs::read_to_string(&tracked).expect("read tracked file");

        assert!(
            result.is_err() && content == "dirty\n",
            "option-like branch result was {result:?}; tracked content was {content:?}"
        );
    }

    #[test]
    fn revision_that_is_not_a_local_branch_name_is_rejected() {
        let repository = initialized_repository();

        let result = checkout_local_branch(repository.path(), "HEAD");

        assert!(
            result.is_err(),
            "revision expression was accepted as a local branch: {result:?}"
        );
    }

    #[test]
    fn existing_local_branch_can_be_checked_out() {
        let repository = initialized_repository();
        let cwd = repository.path();
        run_git(cwd, &["branch", "feature/test"]);

        checkout_local_branch(cwd, "feature/test").expect("checkout local branch");

        assert_eq!(
            git_output(cwd, &["branch", "--show-current"]).expect("read current branch"),
            "feature/test"
        );
    }
}

#[tauri::command]
pub async fn acp_git_info(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<AcpGitInfo, String> {
    let project = acp_repo::get_project(&state.sea_db, &project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    let cwd = PathBuf::from(&project.root_path);

    // Not a git repo → soft empty result
    let is_repo = std::process::Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(&cwd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !is_repo {
        return Ok(AcpGitInfo {
            branch: None,
            branches: vec![],
            is_repo: false,
        });
    }

    let branch = git_output(&cwd, &["branch", "--show-current"]).ok();
    let branch = branch.filter(|b| !b.is_empty());

    // Local branches (no remote-only clutter)
    let raw = git_output(&cwd, &["branch", "--format=%(refname:short)"]).unwrap_or_default();
    let mut branches: Vec<String> = raw
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    branches.sort();
    branches.dedup();

    Ok(AcpGitInfo {
        branch,
        branches,
        is_repo: true,
    })
}

#[tauri::command]
pub async fn acp_git_checkout(
    state: State<'_, AppState>,
    project_id: String,
    branch: String,
) -> Result<AcpGitInfo, String> {
    let project = acp_repo::get_project(&state.sea_db, &project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    let cwd = PathBuf::from(&project.root_path);
    checkout_local_branch(&cwd, &branch)?;
    acp_git_info(state, project_id).await
}
