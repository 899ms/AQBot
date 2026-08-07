//! ACP runtime: spawn external agents and run prompt turns.
//!
//! Live agent processes are kept per `session_key` (AQBot thread id) so multi-turn
//! prompts reuse the same process. After process death / app restart we try
//! `session/load`, then fall back to `session/new` — never prompt with a bare
//! stale session id (that caused "Session … not found").

use crate::config::{shell_command_line, ConfiguredAgent};
use agent_client_protocol::schema::v1::{
    ContentBlock, InitializeRequest, LoadSessionRequest, NewSessionRequest, PromptRequest,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionId, SessionNotification, TextContent,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{AcpAgent, Agent, ConnectionTo, Responder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, oneshot};

/// UI-facing events emitted during a prompt turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AcpEvent {
    #[serde(rename_all = "camelCase")]
    StreamText { text: String },
    #[serde(rename_all = "camelCase")]
    StreamThinking { thinking: String },
    #[serde(rename_all = "camelCase")]
    ToolCall {
        tool_call_id: String,
        title: Option<String>,
        kind: Option<String>,
        status: Option<String>,
        raw: serde_json::Value,
    },
    #[serde(rename_all = "camelCase")]
    ToolCallUpdate {
        tool_call_id: String,
        status: Option<String>,
        raw: serde_json::Value,
    },
    #[serde(rename_all = "camelCase")]
    Plan { raw: serde_json::Value },
    #[serde(rename_all = "camelCase")]
    PermissionRequest {
        request_id: String,
        raw: serde_json::Value,
        options: Vec<PermissionOptionView>,
    },
    #[serde(rename_all = "camelCase")]
    Status { message: String },
    #[serde(rename_all = "camelCase")]
    Error { message: String },
    #[serde(rename_all = "camelCase")]
    Done {
        stop_reason: String,
        session_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOptionView {
    pub option_id: String,
    pub name: String,
    pub kind: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PromptOutcome {
    pub session_id: String,
    pub stop_reason: String,
    pub assistant_text: String,
}

type PermissionMap = Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>;
type EventTxSlot = Arc<Mutex<Option<mpsc::UnboundedSender<AcpEvent>>>>;

struct PromptJob {
    prompt: String,
    preferred_session_id: Option<String>,
    event_tx: mpsc::UnboundedSender<AcpEvent>,
    reply: oneshot::Sender<anyhow::Result<PromptOutcome>>,
}

struct LiveSession {
    job_tx: mpsc::UnboundedSender<PromptJob>,
    agent_id: String,
    cwd: PathBuf,
    /// Resolves once initialize() has completed (or failed).
    ready: Mutex<Option<oneshot::Receiver<anyhow::Result<()>>>>,
}

/// Shared runtime handle for the app.
pub struct AcpRuntime {
    permissions: PermissionMap,
    sessions: Mutex<HashMap<String, LiveSession>>,
}

impl Default for AcpRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl AcpRuntime {
    pub fn new() -> Self {
        Self {
            permissions: Arc::new(Mutex::new(HashMap::new())),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub async fn resolve_permission(&self, request_id: &str, option_id: String) -> bool {
        let mut map = self.permissions.lock().await;
        if let Some(tx) = map.remove(request_id) {
            let _ = tx.send(option_id);
            true
        } else {
            false
        }
    }

    /// Drop a live agent process (e.g. thread deleted).
    pub async fn drop_session(&self, session_key: &str) {
        self.sessions.lock().await.remove(session_key);
    }

    /// Run a prompt turn, reusing a live agent process when possible.
    ///
    /// - `session_key`: AQBot thread id (stable live-process key)
    /// - `preferred_session_id`: last known ACP session id from DB
    pub async fn prompt(
        &self,
        session_key: &str,
        agent: &ConfiguredAgent,
        cwd: PathBuf,
        prompt: String,
        preferred_session_id: Option<String>,
        auto_approve: bool,
        event_tx: mpsc::UnboundedSender<AcpEvent>,
    ) -> anyhow::Result<PromptOutcome> {
        let _ = event_tx.send(AcpEvent::Status {
            message: format!("Starting agent: {}", agent.name),
        });

        self.ensure_live(session_key, agent, cwd.clone(), auto_approve, &event_tx)
            .await?;

        let (reply_tx, reply_rx) = oneshot::channel();
        if self
            .try_send_job(
                session_key,
                PromptJob {
                    prompt: prompt.clone(),
                    preferred_session_id: preferred_session_id.clone(),
                    event_tx: event_tx.clone(),
                    reply: reply_tx,
                },
            )
            .await
            .is_err()
        {
            // Worker died — restart once and retry.
            self.sessions.lock().await.remove(session_key);
            self.ensure_live(session_key, agent, cwd, auto_approve, &event_tx)
                .await?;
            let (reply_tx, reply_rx2) = oneshot::channel();
            self.try_send_job(
                session_key,
                PromptJob {
                    prompt,
                    preferred_session_id,
                    event_tx: event_tx.clone(),
                    reply: reply_tx,
                },
            )
            .await
            .map_err(|_| anyhow::anyhow!("agent session closed after restart"))?;
            return self.await_reply(session_key, reply_rx2).await;
        }

        self.await_reply(session_key, reply_rx).await
    }

    async fn await_reply(
        &self,
        session_key: &str,
        reply_rx: oneshot::Receiver<anyhow::Result<PromptOutcome>>,
    ) -> anyhow::Result<PromptOutcome> {
        match reply_rx.await {
            Ok(result) => {
                if result.is_err() {
                    self.sessions.lock().await.remove(session_key);
                }
                result
            }
            Err(_) => {
                self.sessions.lock().await.remove(session_key);
                anyhow::bail!("agent session worker exited")
            }
        }
    }

    async fn try_send_job(&self, session_key: &str, job: PromptJob) -> Result<(), ()> {
        let map = self.sessions.lock().await;
        let live = map.get(session_key).ok_or(())?;
        live.job_tx.send(job).map_err(|_| ())
    }

    async fn ensure_live(
        &self,
        session_key: &str,
        agent: &ConfiguredAgent,
        cwd: PathBuf,
        auto_approve: bool,
        event_tx: &mpsc::UnboundedSender<AcpEvent>,
    ) -> anyhow::Result<()> {
        let mut map = self.sessions.lock().await;
        let needs_new = match map.get(session_key) {
            None => true,
            Some(s) => s.agent_id != agent.id || s.cwd != cwd || s.job_tx.is_closed(),
        };
        if !needs_new {
            return Ok(());
        }
        map.remove(session_key);
        let _ = event_tx.send(AcpEvent::Status {
            message: "Launching agent process…".into(),
        });
        let live = spawn_live_session(agent, cwd, auto_approve, self.permissions.clone())?;
        // Wait for initialize before accepting prompts (avoids lost jobs on failed spawn).
        let ready_rx = {
            let mut ready = live.ready.lock().await;
            ready.take()
        };
        map.insert(session_key.to_string(), live);
        drop(map);

        if let Some(ready_rx) = ready_rx {
            match tokio::time::timeout(std::time::Duration::from_secs(120), ready_rx).await {
                Ok(Ok(Ok(()))) => {
                    let _ = event_tx.send(AcpEvent::Status {
                        message: "Agent ready".into(),
                    });
                }
                Ok(Ok(Err(e))) => {
                    self.sessions.lock().await.remove(session_key);
                    return Err(e);
                }
                Ok(Err(_)) => {
                    self.sessions.lock().await.remove(session_key);
                    anyhow::bail!("agent process exited during startup");
                }
                Err(_) => {
                    self.sessions.lock().await.remove(session_key);
                    anyhow::bail!("agent initialize timed out");
                }
            }
        }
        Ok(())
    }
}

/// Pull a human-readable reason out of agent-client-protocol / npm spawn errors.
fn summarize_agent_spawn_error(raw: &str) -> String {
    // Prefer the nested "data": "Process exited … npm error …" payload when present.
    if let Some(idx) = raw.find("npm error") {
        let slice = &raw[idx..];
        let cleaned = slice
            .replace("\\n", "\n")
            .replace("\\\"", "\"")
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(4)
            .collect::<Vec<_>>()
            .join(" ");
        if !cleaned.is_empty() {
            return cleaned.chars().take(400).collect();
        }
    }
    if let Some(idx) = raw.find("Process exited") {
        return raw[idx..].chars().take(400).collect();
    }
    let trimmed = raw.trim();
    if trimmed.len() > 400 {
        format!("{}…", &trimmed[..400])
    } else if trimmed.is_empty() {
        "unknown error".into()
    } else {
        trimmed.to_string()
    }
}

fn spawn_live_session(
    agent: &ConfiguredAgent,
    cwd: PathBuf,
    auto_approve: bool,
    permissions: PermissionMap,
) -> anyhow::Result<LiveSession> {
    let cmd_line = shell_command_line(agent);
    let acp_agent = AcpAgent::from_str(&cmd_line)
        .map_err(|e| anyhow::anyhow!("failed to parse agent command `{cmd_line}`: {e}"))?;

    let (job_tx, mut job_rx) = mpsc::unbounded_channel::<PromptJob>();
    let (ready_tx, ready_rx) = oneshot::channel::<anyhow::Result<()>>();
    let agent_id = agent.id.clone();
    let agent_name = agent.name.clone();
    let cwd_for_worker = cwd.clone();
    let event_slot: EventTxSlot = Arc::new(Mutex::new(None));

    tokio::spawn(async move {
        let event_slot_notif = event_slot.clone();
        let event_slot_perm = event_slot.clone();
        let event_slot_jobs = event_slot.clone();
        let permissions_perm = permissions.clone();
        // Shared so connect failure can still surface a useful error to ensure_live
        // (process exit before initialize used to drop the oneshot → generic message).
        let ready_tx = Arc::new(Mutex::new(Some(ready_tx)));

        let connect_result = agent_client_protocol::Client
            .builder()
            .name("aqbot")
            .on_receive_notification(
                {
                    let event_slot = event_slot_notif;
                    // Sync FnMut → async block so the handler can be re-entered.
                    move |notification: SessionNotification, _cx| {
                        let event_slot = event_slot.clone();
                        async move {
                            let tx = event_slot.lock().await.clone();
                            if let Some(tx) = tx {
                                map_session_notification(&notification, &tx);
                            }
                            Ok(())
                        }
                    }
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .on_receive_request(
                {
                    let permissions = permissions_perm;
                    let event_slot = event_slot_perm;
                    let auto = auto_approve;
                    move |request: RequestPermissionRequest, responder, _connection| {
                        let permissions = permissions.clone();
                        let event_slot = event_slot.clone();
                        async move {
                            let event_tx = event_slot.lock().await.clone();
                            handle_permission_request(
                                request,
                                responder,
                                auto,
                                permissions,
                                event_tx,
                            )
                            .await
                        }
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .connect_with(acp_agent, {
                let event_slot = event_slot_jobs;
                let ready_tx = ready_tx.clone();
                move |connection: ConnectionTo<Agent>| {
                    let event_slot = event_slot.clone();
                    let cwd = cwd_for_worker.clone();
                    let ready_tx = ready_tx.clone();
                    async move {
                        // Initialize once per process.
                        let init_result = connection
                            .send_request(InitializeRequest::new(ProtocolVersion::V1))
                            .block_task()
                            .await;

                        match init_result {
                            Ok(_) => {
                                if let Some(tx) = ready_tx.lock().await.take() {
                                    let _ = tx.send(Ok(()));
                                }
                            }
                            Err(e) => {
                                let msg = format!("initialize failed: {e}");
                                if let Some(tx) = ready_tx.lock().await.take() {
                                    let _ = tx.send(Err(anyhow::anyhow!(msg.clone())));
                                }
                                return Err(agent_client_protocol::util::internal_error(msg));
                            }
                        }

                        let mut active_session: Option<SessionId> = None;

                        while let Some(job) = job_rx.recv().await {
                            // Route streaming notifications to this turn's event channel.
                            *event_slot.lock().await = Some(job.event_tx.clone());

                            let result = run_one_prompt(
                                &connection,
                                &cwd,
                                &job.prompt,
                                job.preferred_session_id.as_deref(),
                                &mut active_session,
                                &job.event_tx,
                            )
                            .await;

                            // Clear turn event routing.
                            *event_slot.lock().await = None;

                            if let Ok(ref outcome) = result {
                                // Keep process-local session id for next turn.
                                active_session = Some(SessionId::new(outcome.session_id.as_str()));
                            }

                            let _ = job.reply.send(result);
                        }

                        Ok(())
                    }
                }
            })
            .await;

        if let Err(e) = connect_result {
            let detail = summarize_agent_spawn_error(&e.to_string());
            tracing::warn!(
                error = %e,
                agent = %agent_name,
                "acp live session exited"
            );
            // Unblock ensure_live with the real failure (e.g. npm ETARGET) when
            // the process died before initialize could send on ready_tx.
            if let Some(tx) = ready_tx.lock().await.take() {
                let _ = tx.send(Err(anyhow::anyhow!(
                    "agent process exited during startup: {detail}"
                )));
            }
        }
    });

    Ok(LiveSession {
        job_tx,
        agent_id,
        cwd,
        ready: Mutex::new(Some(ready_rx)),
    })
}

async fn run_one_prompt(
    connection: &ConnectionTo<Agent>,
    cwd: &PathBuf,
    prompt: &str,
    preferred_session_id: Option<&str>,
    active_session: &mut Option<SessionId>,
    event_tx: &mpsc::UnboundedSender<AcpEvent>,
) -> anyhow::Result<PromptOutcome> {
    // Prefer in-process session (multi-turn on same process).
    let mut session_id = if let Some(sid) = active_session.clone() {
        sid
    } else if let Some(preferred) = preferred_session_id {
        // After app refresh: try session/load, then new.
        let _ = event_tx.send(AcpEvent::Status {
            message: "Restoring session…".into(),
        });
        let preferred_owned = preferred.to_string();
        match connection
            .send_request(LoadSessionRequest::new(
                SessionId::new(preferred_owned.as_str()),
                cwd.clone(),
            ))
            .block_task()
            .await
        {
            Ok(_loaded) => {
                let _ = event_tx.send(AcpEvent::Status {
                    message: "Session restored".into(),
                });
                SessionId::new(preferred_owned)
            }
            Err(e) => {
                tracing::info!(
                    error = %e,
                    session = %preferred_owned,
                    "session/load failed; creating new session"
                );
                let _ = event_tx.send(AcpEvent::Status {
                    message: "Creating session…".into(),
                });
                let new_session = connection
                    .send_request(NewSessionRequest::new(cwd.clone()))
                    .block_task()
                    .await
                    .map_err(|e| anyhow::anyhow!("session/new failed: {e}"))?;
                new_session.session_id
            }
        }
    } else {
        let _ = event_tx.send(AcpEvent::Status {
            message: "Creating session…".into(),
        });
        let new_session = connection
            .send_request(NewSessionRequest::new(cwd.clone()))
            .block_task()
            .await
            .map_err(|e| anyhow::anyhow!("session/new failed: {e}"))?;
        new_session.session_id
    };

    let session_id_str = session_id.to_string();
    let _ = event_tx.send(AcpEvent::Status {
        message: "Sending prompt…".into(),
    });

    let prompt_result = connection
        .send_request(PromptRequest::new(
            session_id.clone(),
            vec![ContentBlock::Text(TextContent::new(prompt.to_string()))],
        ))
        .block_task()
        .await;

    let prompt_response = match prompt_result {
        Ok(r) => r,
        Err(e) => {
            let msg = e.to_string();
            // Stale session after partial load / agent restart mid-process.
            if msg.to_lowercase().contains("not found") || msg.contains("session") {
                let _ = event_tx.send(AcpEvent::Status {
                    message: "Session expired, creating new…".into(),
                });
                let new_session = connection
                    .send_request(NewSessionRequest::new(cwd.clone()))
                    .block_task()
                    .await
                    .map_err(|e2| anyhow::anyhow!("session/new failed after not-found: {e2}"))?;
                session_id = new_session.session_id;
                *active_session = Some(session_id.clone());
                connection
                    .send_request(PromptRequest::new(
                        session_id.clone(),
                        vec![ContentBlock::Text(TextContent::new(prompt.to_string()))],
                    ))
                    .block_task()
                    .await
                    .map_err(|e2| anyhow::anyhow!("session/prompt failed: {e2}"))?
            } else {
                return Err(anyhow::anyhow!("session/prompt failed: {msg}"));
            }
        }
    };

    let stop_reason = format!("{:?}", prompt_response.stop_reason);
    let final_session = session_id.to_string();
    // Brief grace so late agent_message_chunk notifications flush into the accumulator
    // before the command layer persists + emits the authoritative acp-done.
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    let _ = session_id_str;
    let _ = event_tx; // final Done is emitted by commands after DB persist

    Ok(PromptOutcome {
        session_id: final_session,
        stop_reason,
        assistant_text: String::new(),
    })
}

async fn handle_permission_request(
    request: RequestPermissionRequest,
    responder: Responder<RequestPermissionResponse>,
    auto: bool,
    permissions: PermissionMap,
    event_tx: Option<mpsc::UnboundedSender<AcpEvent>>,
) -> Result<(), agent_client_protocol::Error> {
    if auto {
        let option_id = request.options.first().map(|opt| opt.option_id.clone());
        if let Some(id) = option_id {
            let _ = responder.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(id)),
            ));
        } else {
            let _ = responder.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Cancelled,
            ));
        }
        return Ok(());
    }

    let request_id = uuid::Uuid::new_v4().to_string();
    let options: Vec<PermissionOptionView> = request
        .options
        .iter()
        .map(|o| PermissionOptionView {
            option_id: o.option_id.to_string(),
            name: o.name.clone(),
            kind: Some(format!("{:?}", o.kind)),
        })
        .collect();

    let raw = serde_json::to_value(&request).unwrap_or_else(|_| serde_json::json!({}));
    let (tx, rx) = oneshot::channel::<String>();
    {
        let mut map = permissions.lock().await;
        map.insert(request_id.clone(), tx);
    }

    if let Some(event_tx) = event_tx {
        let _ = event_tx.send(AcpEvent::PermissionRequest {
            request_id: request_id.clone(),
            raw,
            options,
        });
    }

    let selected = tokio::time::timeout(std::time::Duration::from_secs(600), rx).await;
    match selected {
        Ok(Ok(option_id)) => {
            let _ = responder.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id)),
            ));
        }
        _ => {
            permissions.lock().await.remove(&request_id);
            let _ = responder.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Cancelled,
            ));
        }
    }
    Ok(())
}

fn map_session_notification(
    notification: &SessionNotification,
    event_tx: &mpsc::UnboundedSender<AcpEvent>,
) {
    let update = &notification.update;
    let value = match serde_json::to_value(update) {
        Ok(v) => v,
        Err(_) => return,
    };

    let kind = value
        .get("sessionUpdate")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match kind {
        "agent_message_chunk" | "user_message_chunk" => {
            if let Some(text) = extract_text_content(&value) {
                let _ = event_tx.send(AcpEvent::StreamText { text });
            }
        }
        "agent_thought_chunk" => {
            if let Some(text) = extract_text_content(&value) {
                let _ = event_tx.send(AcpEvent::StreamThinking { thinking: text });
            }
        }
        "tool_call" => {
            let tool_call_id = value
                .get("toolCallId")
                .or_else(|| value.get("tool_call_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let title = value
                .get("title")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let kind = value
                .get("kind")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let status = value
                .get("status")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let _ = event_tx.send(AcpEvent::ToolCall {
                tool_call_id,
                title,
                kind,
                status,
                raw: value,
            });
        }
        "tool_call_update" => {
            let tool_call_id = value
                .get("toolCallId")
                .or_else(|| value.get("tool_call_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let status = value
                .get("status")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let _ = event_tx.send(AcpEvent::ToolCallUpdate {
                tool_call_id,
                status,
                raw: value,
            });
        }
        "plan" => {
            let _ = event_tx.send(AcpEvent::Plan { raw: value });
        }
        _ => {
            tracing::debug!(%kind, "acp session update");
        }
    }
}

fn extract_text_content(value: &serde_json::Value) -> Option<String> {
    if let Some(c) = value.get("content") {
        if let Some(t) = c.get("text").and_then(|v| v.as_str()) {
            return Some(t.to_string());
        }
        if let Some(t) = c.as_str() {
            return Some(t.to_string());
        }
    }
    value
        .get("text")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}
