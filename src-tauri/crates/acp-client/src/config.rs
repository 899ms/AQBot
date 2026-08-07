//! User ACP agent configuration: `~/.aqbot/acp/agents.toml`

use crate::paths::{agents_toml_path, ensure_acp_dirs};
use crate::registry::{resolve_launch, RegistryAgent};
use crate::types::AgentProbeResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpAgentsFile {
    #[serde(default)]
    pub general: AcpGeneralConfig,
    #[serde(default)]
    pub agents: Vec<ConfiguredAgent>,
}

impl Default for AcpAgentsFile {
    fn default() -> Self {
        Self {
            general: AcpGeneralConfig::default(),
            agents: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpGeneralConfig {
    #[serde(default = "default_idle")]
    pub idle_timeout_secs: u64,
    #[serde(default = "default_max_proc")]
    pub max_concurrent_processes: u32,
    /// prompt | default | accept_edits | auto_approve | full_access
    #[serde(default = "default_permission")]
    pub permission_default: String,
    /// on_start | manual | never
    #[serde(default = "default_refresh")]
    pub registry_refresh: String,
}

fn default_idle() -> u64 {
    1800
}
/// 0 = unlimited concurrent agent processes.
fn default_max_proc() -> u32 {
    0
}
fn default_permission() -> String {
    "prompt".into()
}
fn default_refresh() -> String {
    "on_start".into()
}

impl Default for AcpGeneralConfig {
    fn default() -> Self {
        Self {
            idle_timeout_secs: default_idle(),
            max_concurrent_processes: default_max_proc(),
            permission_default: default_permission(),
            registry_refresh: default_refresh(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfiguredAgent {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    /// registry | custom
    #[serde(default = "default_source")]
    pub source: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub sort: i32,
}

fn default_source() -> String {
    "registry".into()
}

pub fn load_agents_file() -> anyhow::Result<AcpAgentsFile> {
    ensure_acp_dirs()?;
    let path = agents_toml_path();
    if !path.exists() {
        let file = AcpAgentsFile::default();
        save_agents_file(&file)?;
        return Ok(file);
    }
    let text = std::fs::read_to_string(&path)?;
    let file: AcpAgentsFile = toml::from_str(&text)?;
    Ok(file)
}

pub fn save_agents_file(file: &AcpAgentsFile) -> anyhow::Result<()> {
    ensure_acp_dirs()?;
    let text = toml::to_string_pretty(file)?;
    std::fs::write(agents_toml_path(), text)?;
    Ok(())
}

pub fn enabled_agents(file: &AcpAgentsFile) -> Vec<&ConfiguredAgent> {
    let mut list: Vec<_> = file.agents.iter().filter(|a| a.enabled).collect();
    list.sort_by_key(|a| a.sort);
    list
}

/// Add or update agent from registry entry (enables by default).
pub fn upsert_from_registry(
    file: &mut AcpAgentsFile,
    agent: &RegistryAgent,
    enabled: bool,
) -> anyhow::Result<()> {
    let launch = resolve_launch(agent)
        .ok_or_else(|| anyhow::anyhow!("no launch method for agent {}", agent.id))?;
    // Do not persist registry CDN icon URLs — the UI resolves brand Color icons
    // from agent id/name (Codex Avatar is white-on-white and CDN SVGs often mismatch).
    // Keep a pre-existing *custom* icon if the user already set one.
    if let Some(existing) = file.agents.iter_mut().find(|a| a.id == agent.id) {
        existing.name = agent.name.clone();
        existing.command = launch.command;
        existing.args = launch.args;
        existing.env = launch.env;
        // Drop auto official-registry CDN icons only (keep user emoji/file/custom url)
        if existing
            .icon
            .as_ref()
            .is_some_and(|u| u.contains("cdn.agentclientprotocol.com"))
        {
            existing.icon = None;
        }
        existing.source = "registry".into();
        existing.enabled = enabled;
    } else {
        let sort = file.agents.len() as i32;
        file.agents.push(ConfiguredAgent {
            id: agent.id.clone(),
            name: agent.name.clone(),
            enabled,
            source: "registry".into(),
            command: launch.command,
            args: launch.args,
            env: launch.env,
            icon: None,
            sort,
        });
    }
    Ok(())
}

pub fn set_agent_enabled(file: &mut AcpAgentsFile, agent_id: &str, enabled: bool) -> bool {
    if let Some(a) = file.agents.iter_mut().find(|a| a.id == agent_id) {
        a.enabled = enabled;
        true
    } else {
        false
    }
}

/// Reorder agents by the given id sequence. Unknown ids are appended at the end.
pub fn reorder_agents(file: &mut AcpAgentsFile, agent_ids: &[String]) {
    let mut by_id: HashMap<String, ConfiguredAgent> = file
        .agents
        .drain(..)
        .map(|a| (a.id.clone(), a))
        .collect();
    let mut ordered = Vec::with_capacity(by_id.len());
    for (i, id) in agent_ids.iter().enumerate() {
        if let Some(mut a) = by_id.remove(id) {
            a.sort = i as i32;
            ordered.push(a);
        }
    }
    // Preserve any agents not present in the id list (should be rare).
    let mut rest: Vec<_> = by_id.into_values().collect();
    rest.sort_by_key(|a| a.sort);
    let base = ordered.len() as i32;
    for (i, mut a) in rest.into_iter().enumerate() {
        a.sort = base + i as i32;
        ordered.push(a);
    }
    file.agents = ordered;
}

pub fn remove_agent(file: &mut AcpAgentsFile, agent_id: &str) -> bool {
    let before = file.agents.len();
    file.agents.retain(|a| a.id != agent_id);
    if file.agents.len() != before {
        for (i, a) in file.agents.iter_mut().enumerate() {
            a.sort = i as i32;
        }
        true
    } else {
        false
    }
}

/// Lightweight availability probe (does not start full ACP session).
pub fn probe_agent(agent: &ConfiguredAgent) -> AgentProbeResult {
    let cmd_display = format!("{} {}", agent.command, agent.args.join(" "));
    // Check command exists on PATH
    let which = if cfg!(windows) { "where" } else { "which" };
    let available = Command::new(which)
        .arg(&agent.command)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    let message = if available {
        format!("Found `{}` on PATH", agent.command)
    } else {
        format!(
            "`{}` not found on PATH. Install the agent CLI, then re-check.",
            agent.command
        )
    };

    AgentProbeResult {
        agent_id: agent.id.clone(),
        available,
        command: cmd_display.trim().to_string(),
        message,
    }
}

pub fn shell_command_line(agent: &ConfiguredAgent) -> String {
    let mut parts = vec![agent.command.clone()];
    parts.extend(agent.args.iter().cloned());
    // Simple quoting for display / AcpAgent::from_str
    parts
        .into_iter()
        .map(|p| {
            if p.contains(' ') {
                format!("\"{p}\"")
            } else {
                p
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
