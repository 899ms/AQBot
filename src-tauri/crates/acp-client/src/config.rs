//! User ACP agent configuration: `~/.aqbot/acp/agents.toml`

use crate::paths::{agents_toml_path, ensure_acp_dirs};
use crate::registry::{resolve_launch, RegistryAgent, RegistryFile};
use crate::types::AgentProbeResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Component, Path};
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

impl AcpAgentsFile {
    pub fn validate(&self) -> anyhow::Result<()> {
        const PERMISSIONS: &[&str] = &[
            "prompt",
            "default",
            "accept_edits",
            "auto_approve",
            "full_access",
        ];
        const REFRESH_POLICIES: &[&str] = &["on_start", "manual", "never"];
        if !PERMISSIONS.contains(&self.general.permission_default.as_str()) {
            anyhow::bail!(
                "invalid ACP permission_default `{}`",
                self.general.permission_default
            );
        }
        if !REFRESH_POLICIES.contains(&self.general.registry_refresh.as_str()) {
            anyhow::bail!(
                "invalid ACP registry_refresh `{}`",
                self.general.registry_refresh
            );
        }

        let mut ids = std::collections::HashSet::new();
        for agent in &self.agents {
            agent.validate()?;
            if !ids.insert(agent.id.as_str()) {
                anyhow::bail!("duplicate ACP agent id `{}`", agent.id);
            }
        }
        Ok(())
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

impl ConfiguredAgent {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.id.trim().is_empty() {
            anyhow::bail!("ACP agent id must not be empty");
        }
        if self.name.trim().is_empty() {
            anyhow::bail!("ACP agent `{}` name must not be empty", self.id);
        }
        if self.command.trim().is_empty() {
            anyhow::bail!("ACP agent `{}` command must not be empty", self.id);
        }
        if self.command.contains('\0') || self.args.iter().any(|arg| arg.contains('\0')) {
            anyhow::bail!("ACP agent `{}` command contains a NUL byte", self.id);
        }
        if self
            .env
            .iter()
            .any(|(key, value)| key.is_empty() || key.contains(['=', '\0']) || value.contains('\0'))
        {
            anyhow::bail!("ACP agent `{}` has an invalid environment entry", self.id);
        }
        Ok(())
    }
}

fn default_source() -> String {
    "registry".into()
}

fn apply_resolved_launch(
    agent: &mut ConfiguredAgent,
    launch: crate::registry::ResolvedLaunch,
) -> bool {
    if agent.command == launch.command && agent.args == launch.args && agent.env == launch.env {
        return false;
    }
    agent.command = launch.command;
    agent.args = launch.args;
    agent.env = launch.env;
    true
}

fn is_persisted_npx_cache_bin(command: &str) -> bool {
    let path = Path::new(command);
    if !path.is_absolute() {
        return false;
    }
    let parts = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part),
            _ => None,
        })
        .collect::<Vec<_>>();
    parts.iter().enumerate().any(|(index, part)| {
        let Some(hash) = parts.get(index + 1).and_then(|part| part.to_str()) else {
            return false;
        };
        *part == "_npx"
            && hash.len() == 16
            && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            && parts
                .get(index + 2)
                .is_some_and(|part| *part == "node_modules")
            && parts.get(index + 3).is_some_and(|part| *part == ".bin")
            && index + 5 == parts.len()
    })
}

fn has_missing_registry_npx_bin(agent: &ConfiguredAgent) -> bool {
    agent.source == "registry"
        && is_persisted_npx_cache_bin(&agent.command)
        && !Path::new(&agent.command).is_file()
}

fn repair_missing_registry_npx_bins_with(
    file: &mut AcpAgentsFile,
    registry: &RegistryFile,
    resolve_registry_agent: impl Fn(&RegistryAgent) -> Option<crate::registry::ResolvedLaunch>,
) -> bool {
    let mut updated = false;
    for configured in file
        .agents
        .iter_mut()
        .filter(|agent| has_missing_registry_npx_bin(agent))
    {
        let Some(registry_agent) = registry
            .agents
            .iter()
            .find(|agent| agent.id == configured.id)
        else {
            tracing::warn!(agent = %configured.id, "missing Registry entry for stale npx cache launch");
            continue;
        };
        let Some(launch) = resolve_registry_agent(registry_agent) else {
            tracing::warn!(agent = %configured.id, "Registry entry cannot repair stale npx cache launch");
            continue;
        };
        updated |= apply_resolved_launch(configured, launch);
    }
    updated
}

fn normalize_loaded_agents_with(
    file: &mut AcpAgentsFile,
    resolve_registry_launch: impl Fn(&ConfiguredAgent) -> Option<crate::registry::ResolvedLaunch>,
) -> bool {
    let mut updated = false;
    for agent in &mut file.agents {
        if agent.enabled
            && agent.source == "registry"
            && crate::registry::official_quarantine_reason(&agent.id).is_some()
        {
            agent.enabled = false;
            updated = true;
        }
        if agent.source != "registry" {
            continue;
        }
        let Some(launch) = resolve_registry_launch(agent) else {
            continue;
        };
        updated |= apply_resolved_launch(agent, launch);
    }
    updated
}

fn normalize_loaded_agents(file: &mut AcpAgentsFile) -> anyhow::Result<bool> {
    let mut updated = normalize_loaded_agents_with(file, |agent| {
        crate::registry::resolve_configured_npx_trampoline(&agent.command, &agent.args, &agent.env)
    });
    if !file.agents.iter().any(has_missing_registry_npx_bin) {
        return Ok(updated);
    }
    let registry = crate::registry::load_registry()?;
    updated |= repair_missing_registry_npx_bins_with(file, &registry, resolve_launch);
    Ok(updated)
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
    let mut file: AcpAgentsFile = toml::from_str(&text)?;
    file.validate()?;
    if normalize_loaded_agents(&mut file)? {
        save_agents_file(&file)?;
    }
    Ok(file)
}

pub fn save_agents_file(file: &AcpAgentsFile) -> anyhow::Result<()> {
    file.validate()?;
    ensure_acp_dirs()?;
    let text = toml::to_string_pretty(file)?;
    let path = agents_toml_path();
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> anyhow::Result<()> {
        let mut output = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        output.write_all(text.as_bytes())?;
        output.sync_all()?;
        std::fs::rename(&temporary, &path)?;
        Ok(())
    })();
    if let Err(error) = result {
        return match std::fs::remove_file(&temporary) {
            Ok(()) => Err(error),
            Err(cleanup) if cleanup.kind() == std::io::ErrorKind::NotFound => Err(error),
            Err(cleanup) => Err(anyhow::anyhow!(
                "{error}; ACP config temporary-file cleanup failed: {cleanup}"
            )),
        };
    }
    Ok(())
}

pub fn enabled_agents(file: &AcpAgentsFile) -> Vec<&ConfiguredAgent> {
    let mut list: Vec<_> = file
        .agents
        .iter()
        .filter(|agent| is_agent_enabled(agent))
        .collect();
    list.sort_by_key(|a| a.sort);
    list
}

pub fn is_agent_enabled(agent: &ConfiguredAgent) -> bool {
    agent.enabled
        && !(agent.source == "registry"
            && crate::registry::official_quarantine_reason(&agent.id).is_some())
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

/// Refresh launch metadata for configured Registry-managed agents while
/// preserving local enablement, order, and custom agents.
pub fn sync_configured_registry_agents(
    file: &mut AcpAgentsFile,
    registry: &RegistryFile,
) -> anyhow::Result<usize> {
    let configured = file
        .agents
        .iter()
        .filter(|agent| agent.source == "registry")
        .map(|agent| (agent.id.clone(), agent.enabled))
        .collect::<Vec<_>>();
    let mut synced = 0;
    for (agent_id, enabled) in configured {
        let Some(agent) = registry.agents.iter().find(|agent| agent.id == agent_id) else {
            continue;
        };
        if agent.quarantine_reason.is_some() {
            if let Some(configured) = file.agents.iter_mut().find(|item| item.id == agent_id) {
                configured.enabled = false;
            }
            synced += 1;
            continue;
        }
        upsert_from_registry(file, agent, enabled)?;
        synced += 1;
    }
    Ok(synced)
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
    let mut by_id: HashMap<String, ConfiguredAgent> =
        file.agents.drain(..).map(|a| (a.id.clone(), a)).collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(id: &str) -> ConfiguredAgent {
        ConfiguredAgent {
            id: id.into(),
            name: id.into(),
            enabled: true,
            source: "custom".into(),
            command: "agent-cli".into(),
            args: vec!["acp".into()],
            env: HashMap::new(),
            icon: None,
            sort: 0,
        }
    }

    #[test]
    fn rejects_empty_commands_and_duplicate_agent_ids() {
        let mut invalid_agent = agent("codex");
        invalid_agent.command = "  ".into();
        assert!(invalid_agent.validate().is_err());

        let file = AcpAgentsFile {
            general: AcpGeneralConfig::default(),
            agents: vec![agent("codex"), agent("codex")],
        };
        assert!(file.validate().is_err());
    }

    #[test]
    fn rejects_unknown_general_policy_values() {
        let mut file = AcpAgentsFile::default();
        file.general.permission_default = "invented".into();
        assert!(file.validate().is_err());

        file.general.permission_default = "default".into();
        file.general.registry_refresh = "hourly".into();
        assert!(file.validate().is_err());
    }

    #[test]
    fn loaded_registry_npx_is_migrated_offline_but_custom_launch_is_untouched() {
        let mut managed = agent("github-copilot-cli");
        managed.source = "registry".into();
        managed.command = "npx".into();
        managed.args = vec!["-y".into(), "@github/copilot@1.0.78".into(), "--acp".into()];
        let mut custom = agent("custom-npx");
        custom.command = "npx".into();
        custom.args = vec!["-y".into(), "custom-agent@1.0.0".into()];
        let custom_before = custom.clone();
        let mut file = AcpAgentsFile {
            general: AcpGeneralConfig::default(),
            agents: vec![managed, custom],
        };

        let updated = normalize_loaded_agents_with(&mut file, |_| {
            Some(crate::registry::ResolvedLaunch {
                command: "/verified/npm-cache/node_modules/.bin/copilot".into(),
                args: vec!["--acp".into()],
                env: HashMap::new(),
                kind: "binary".into(),
            })
        });

        assert!(updated);
        assert_eq!(
            file.agents[0].command,
            "/verified/npm-cache/node_modules/.bin/copilot"
        );
        assert_eq!(file.agents[0].args, ["--acp"]);
        assert_eq!(file.agents[1].command, custom_before.command);
        assert_eq!(file.agents[1].args, custom_before.args);
    }

    fn deleted_npx_cache_bin(agent_id: &str, source: &str) -> ConfiguredAgent {
        let mut configured = agent(agent_id);
        configured.source = source.into();
        configured.command = std::env::temp_dir()
            .join(format!("aqbot-deleted-cache-{}", uuid::Uuid::new_v4()))
            .join("_npx/0123456789abcdef/node_modules/.bin/agent-cli")
            .to_string_lossy()
            .into_owned();
        configured
    }

    #[test]
    fn deleted_registry_cache_bin_recovers_to_exact_npx_but_custom_is_untouched() {
        let managed = deleted_npx_cache_bin("github-copilot-cli", "registry");
        let custom = deleted_npx_cache_bin("custom-cache-agent", "custom");
        let custom_command = custom.command.clone();
        let mut file = AcpAgentsFile {
            general: AcpGeneralConfig::default(),
            agents: vec![managed, custom],
        };
        let mut registry = crate::registry::load_builtin_registry().expect("builtin Registry");
        let missing_cache =
            std::env::temp_dir().join(format!("aqbot-empty-npm-cache-{}", uuid::Uuid::new_v4()));
        registry
            .agents
            .iter_mut()
            .find(|agent| agent.id == "github-copilot-cli")
            .and_then(|agent| agent.distribution.as_mut())
            .and_then(|distribution| distribution.npx.as_mut())
            .expect("Copilot npx distribution")
            .env
            .insert(
                "npm_config_cache".into(),
                missing_cache.to_string_lossy().into_owned(),
            );

        let updated = repair_missing_registry_npx_bins_with(&mut file, &registry, resolve_launch);

        assert!(updated);
        assert_eq!(file.agents[0].command, "npx");
        assert_eq!(
            file.agents[0].args.last().map(String::as_str),
            Some("--acp")
        );
        assert_eq!(file.agents[1].command, custom_command);
    }

    #[cfg(unix)]
    #[test]
    fn deleted_registry_cache_bin_can_move_to_a_new_verified_cache_bin() {
        let fixture = crate::registry::NpxCacheFixture::new(
            "@agentclientprotocol/codex-acp",
            "1.1.13",
            "codex-acp",
            "dist/index.js",
        );
        let mut file = AcpAgentsFile {
            general: AcpGeneralConfig::default(),
            agents: vec![deleted_npx_cache_bin("codex-acp", "registry")],
        };
        let mut registry = crate::registry::load_builtin_registry().expect("builtin Registry");
        registry
            .agents
            .iter_mut()
            .find(|agent| agent.id == "codex-acp")
            .and_then(|agent| agent.distribution.as_mut())
            .and_then(|distribution| distribution.npx.as_mut())
            .expect("Codex npx distribution")
            .env
            .insert(
                "npm_config_cache".into(),
                fixture.npm_cache.to_string_lossy().into_owned(),
            );

        let updated = repair_missing_registry_npx_bins_with(&mut file, &registry, resolve_launch);

        assert!(updated);
        assert_eq!(Path::new(&file.agents[0].command), fixture.bin_link);
    }

    #[test]
    fn registry_refresh_updates_only_managed_agents_and_preserves_local_state() {
        let mut managed = agent("codex-acp");
        managed.source = "registry".into();
        managed.enabled = false;
        managed.command = "obsolete-codex-launch".into();
        managed.sort = 7;
        let custom = agent("my-private-agent");
        let mut file = AcpAgentsFile {
            general: AcpGeneralConfig::default(),
            agents: vec![managed, custom.clone()],
        };
        let registry = crate::registry::load_builtin_registry().expect("builtin Registry");

        assert_eq!(
            sync_configured_registry_agents(&mut file, &registry).expect("sync Registry"),
            1
        );
        let managed = file
            .agents
            .iter()
            .find(|agent| agent.id == "codex-acp")
            .expect("managed agent remains configured");
        assert_ne!(managed.command, "obsolete-codex-launch");
        assert!(!managed.enabled);
        assert_eq!(managed.sort, 7);
        assert_eq!(
            file.agents
                .iter()
                .find(|agent| agent.id == custom.id)
                .map(|agent| (&agent.command, &agent.args)),
            Some((&custom.command, &custom.args))
        );
    }

    #[test]
    fn registry_refresh_disables_officially_quarantined_agents() {
        let mut quarantined = agent("fast-agent");
        quarantined.source = "registry".into();
        let mut file = AcpAgentsFile {
            general: AcpGeneralConfig::default(),
            agents: vec![quarantined],
        };
        let registry = crate::registry::load_builtin_registry().expect("builtin Registry");

        assert_eq!(
            sync_configured_registry_agents(&mut file, &registry).expect("sync Registry"),
            1
        );
        assert!(!file.agents[0].enabled);
    }
}
