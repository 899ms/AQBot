//! ACP runtime: spawn external agents and run prompt turns.
//!
//! Live agent processes are kept per `session_key` (AQBot thread id) so multi-turn
//! prompts reuse the same process. After process death / app restart we try
//! `session/load`, then fall back to `session/new` — never prompt with a bare
//! stale session id (that caused "Session … not found").

use crate::config::ConfiguredAgent;
use agent_client_protocol::schema::v1::{
    AgentCapabilities, AgentNotification, BooleanConfigOptionCapabilities, CancelNotification,
    ClientCapabilities, ClientNotification, ClientSessionCapabilities, CloseSessionRequest,
    ContentBlock, CreateElicitationRequest, CreateElicitationResponse, ElicitationAcceptAction,
    ElicitationAction, ElicitationCapabilities, ElicitationContentValue,
    ElicitationFormCapabilities, ElicitationFormMode, ElicitationMode, ElicitationPropertySchema,
    ElicitationSchema, ElicitationScope, ExtNotification, ImageContent, Implementation,
    InitializeRequest, LoadSessionRequest, McpServer, MultiSelectItems, NewSessionResponse,
    PermissionOption, PermissionOptionKind, PromptRequest, RequestPermissionOutcome,
    RequestPermissionResponse, ResourceLink, ResumeSessionRequest, SelectedPermissionOutcome,
    SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory, SessionConfigOptionValue,
    SessionConfigOptionsCapabilities, SessionConfigSelectOption, SessionConfigSelectOptions,
    SessionId, SessionMode, SessionModeId, SessionModeState, SessionNotification, SessionUpdate,
    SetSessionConfigOptionRequest, SetSessionModeRequest, StringFormat, TextContent,
    ToolCallUpdate,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, Agent, ConnectionTo, JsonRpcRequest, JsonRpcResponse, Responder,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering},
    Arc, Mutex as StdMutex, OnceLock,
};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, watch, Mutex};

// Keep the runtime implementation in one Rust module so its concurrency state
// and private protocol helpers retain the same visibility and ordering rules,
// while grouping the source by cohesive maintenance areas.
include!("runtime/public_api.rs");
include!("runtime/interaction_state.rs");
include!("runtime/state.rs");
include!("runtime/lifecycle.rs");
include!("runtime/process.rs");
include!("runtime/interaction_wire.rs");
include!("runtime/session_config.rs");
include!("runtime/prompt.rs");
include!("runtime/interactions.rs");
include!("runtime/notifications.rs");
include!("runtime/tests.rs");
