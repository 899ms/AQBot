//! Thin ACP Client layer for AQBot.
//!
//! - Registry: builtin snapshot + live/cache fetch
//! - Config: `~/.aqbot/acp/agents.toml`
//! - Runtime: spawn external ACP agents over stdio via `agent-client-protocol`

pub mod config;
pub mod paths;
pub mod proxy;
pub mod registry;
pub mod runtime;
pub mod types;

pub use config::{AcpAgentsFile, AcpGeneralConfig, ConfiguredAgent};
pub use registry::{
    load_registry, refresh_registry, RegistryAgent, RegistrySource, ResolvedLaunch,
};
pub use runtime::{
    AcpEvent, AcpPromptAttachment, AcpPromptHandle, AcpPromptInput, AcpRuntime, PromptOutcome,
};
pub use types::*;
