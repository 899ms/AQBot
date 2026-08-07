//! Official ACP Registry loader.
//!
//! Sources (priority for reads after refresh):
//! 1. live CDN (when refresh succeeds)
//! 2. local cache `~/.aqbot/acp/registry.cache.json`
//! 3. builtin snapshot embedded in the binary

use crate::paths::{ensure_acp_dirs, registry_cache_path};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub const REGISTRY_URL: &str =
    "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";

/// Full offline snapshot of the official ACP registry (kept in sync with CDN).
/// Online refresh still updates `~/.aqbot/acp/registry.cache.json` when available.
pub const BUILTIN_REGISTRY_JSON: &str = include_str!("../resources/registry.builtin.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryFile {
    pub version: String,
    pub agents: Vec<RegistryAgent>,
    #[serde(default)]
    pub source: Option<RegistrySource>,
    #[serde(default)]
    pub fetched_at: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RegistrySource {
    Builtin,
    Cache,
    Live,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryAgent {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub distribution: Option<RegistryDistribution>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RegistryDistribution {
    #[serde(default)]
    pub npx: Option<NpxDist>,
    #[serde(default)]
    pub uvx: Option<UvxDist>,
    #[serde(default)]
    pub binary: Option<HashMap<String, BinaryDist>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpxDist {
    pub package: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UvxDist {
    pub package: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryDist {
    #[serde(default)]
    pub archive: Option<String>,
    pub cmd: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLaunch {
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub kind: String,
}

fn current_platform_key() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let os_part = match os {
        "macos" => "darwin",
        "windows" => "windows",
        other => other,
    };
    let arch_part = match arch {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        other => other,
    };
    format!("{os_part}-{arch_part}")
}

/// Resolve a CLI to an absolute path when possible.
/// GUI apps often lack shell-augmented PATH entries like `~/.grok/bin` or nvm,
/// so also probe well-known install locations for common agent CLIs.
fn resolve_command_path(cmd: &str) -> Option<String> {
    // Already absolute / relative with separator
    if cmd.contains('/') || cmd.contains('\\') {
        let p = PathBuf::from(cmd);
        if p.is_file() {
            return Some(cmd.to_string());
        }
    }

    let which = if cfg!(windows) { "where" } else { "which" };
    if let Ok(output) = std::process::Command::new(which).arg(cmd).output() {
        if output.status.success() {
            if let Ok(stdout) = String::from_utf8(output.stdout) {
                if let Some(line) = stdout.lines().next() {
                    let line = line.trim();
                    if !line.is_empty() {
                        return Some(line.to_string());
                    }
                }
            }
        }
    }

    // Well-known install dirs (macOS/Linux) when GUI PATH is minimal.
    if let Some(home) = dirs::home_dir().or_else(|| std::env::var_os("HOME").map(PathBuf::from)) {
        let candidates = [
            home.join(".grok/bin").join(cmd),
            home.join(".local/bin").join(cmd),
            home.join(".cargo/bin").join(cmd),
            PathBuf::from("/opt/homebrew/bin").join(cmd),
            PathBuf::from("/usr/local/bin").join(cmd),
        ];
        for c in candidates {
            if c.is_file() {
                return Some(c.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// Official CDN / older builtin snapshots sometimes pin packages that never
/// published (e.g. `@xai-official/grok@1.0.0`). Rewrite known-bad pins so
/// enable-from-registry and launch stay usable offline and after refresh.
fn normalize_registry(file: &mut RegistryFile) {
    for agent in &mut file.agents {
        if agent.id != "grok-build" {
            continue;
        }
        let Some(dist) = agent.distribution.as_mut() else {
            continue;
        };
        if let Some(npx) = dist.npx.as_mut() {
            // 1.0.0 was advertised by ACP CDN but never published on npm.
            // 0.2.x is the real line; pin a known-good release.
            if npx.package == "@xai-official/grok@1.0.0"
                || npx.package == "@xai-official/grok@1.0"
                || npx.package == "@xai-official/grok"
            {
                npx.package = "@xai-official/grok@0.2.121".into();
            }
            if npx.args.is_empty() {
                npx.args = vec!["agent".into(), "stdio".into()];
            }
        }
        if agent.version.as_deref() == Some("1.0.0") {
            agent.version = Some("0.2.121".into());
        }
    }
}

/// Resolve a launch command for the current platform.
/// Prefer an already-installed binary on PATH when the registry declares one
/// (e.g. local `grok` from the official installer). Otherwise prefer npx/uvx
/// (no manual download); fall back to binary cmd name only (V1 does not install).
pub fn resolve_launch(agent: &RegistryAgent) -> Option<ResolvedLaunch> {
    let dist = agent.distribution.as_ref()?;

    // Prefer local CLI when the user already installed it (faster, no npm pin issues).
    if let Some(bin_map) = &dist.binary {
        let key = current_platform_key();
        if let Some(bin) = bin_map.get(&key) {
            let cmd = PathBuf::from(&bin.cmd)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| bin.cmd.clone());
            if let Some(resolved) = resolve_command_path(&cmd) {
                return Some(ResolvedLaunch {
                    command: resolved,
                    args: bin.args.clone(),
                    env: bin.env.clone(),
                    kind: "binary".into(),
                });
            }
        }
    }

    if let Some(npx) = &dist.npx {
        let mut args = vec!["-y".to_string(), npx.package.clone()];
        args.extend(npx.args.clone());
        return Some(ResolvedLaunch {
            command: "npx".into(),
            args,
            env: npx.env.clone(),
            kind: "npx".into(),
        });
    }

    if let Some(uvx) = &dist.uvx {
        let mut args = vec![uvx.package.clone()];
        args.extend(uvx.args.clone());
        return Some(ResolvedLaunch {
            command: "uvx".into(),
            args,
            env: uvx.env.clone(),
            kind: "uvx".into(),
        });
    }

    if let Some(bin_map) = &dist.binary {
        let key = current_platform_key();
        if let Some(bin) = bin_map.get(&key) {
            // V1: only expose cmd basename; user must install binary themselves.
            let cmd = PathBuf::from(&bin.cmd)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| bin.cmd.clone());
            return Some(ResolvedLaunch {
                command: cmd,
                args: bin.args.clone(),
                env: bin.env.clone(),
                kind: "binary".into(),
            });
        }
    }

    None
}

fn parse_registry(json: &str, source: RegistrySource) -> anyhow::Result<RegistryFile> {
    // Registry CDN uses snake_case in distribution keys; keep flexible parse.
    let mut file: RegistryFile = serde_json::from_str(json).or_else(|_| {
        // CDN uses original camelCase mixed with snake_case field names in distribution.
        // Re-parse with a raw Value and map.
        parse_registry_flexible(json)
    })?;
    file.source = Some(source);
    Ok(file)
}

fn parse_registry_flexible(json: &str) -> anyhow::Result<RegistryFile> {
    #[derive(Deserialize)]
    struct RawFile {
        version: String,
        agents: Vec<serde_json::Value>,
    }
    let raw: RawFile = serde_json::from_str(json)?;
    let mut agents = Vec::new();
    for a in raw.agents {
        let id = a
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            continue;
        }
        let name = a
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&id)
            .to_string();
        let version = a
            .get("version")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let description = a
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let repository = a
            .get("repository")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let website = a
            .get("website")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let icon = a
            .get("icon")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let license = a
            .get("license")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let distribution = a.get("distribution").and_then(|d| {
            let mut dist = RegistryDistribution::default();
            if let Some(npx) = d.get("npx") {
                if let Some(package) = npx.get("package").and_then(|v| v.as_str()) {
                    let args = npx
                        .get("args")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default();
                    let env = npx
                        .get("env")
                        .and_then(|v| v.as_object())
                        .map(|m| {
                            m.iter()
                                .filter_map(|(k, v)| {
                                    v.as_str().map(|s| (k.clone(), s.to_string()))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    dist.npx = Some(NpxDist {
                        package: package.to_string(),
                        args,
                        env,
                    });
                }
            }
            if let Some(uvx) = d.get("uvx") {
                if let Some(package) = uvx.get("package").and_then(|v| v.as_str()) {
                    let args = uvx
                        .get("args")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default();
                    let env = uvx
                        .get("env")
                        .and_then(|v| v.as_object())
                        .map(|m| {
                            m.iter()
                                .filter_map(|(k, v)| {
                                    v.as_str().map(|s| (k.clone(), s.to_string()))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    dist.uvx = Some(UvxDist {
                        package: package.to_string(),
                        args,
                        env,
                    });
                }
            }
            if let Some(bin) = d.get("binary").and_then(|v| v.as_object()) {
                let mut map = HashMap::new();
                for (k, v) in bin {
                    if let Some(cmd) = v.get("cmd").and_then(|c| c.as_str()) {
                        let args = v
                            .get("args")
                            .and_then(|a| a.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let env = v
                            .get("env")
                            .and_then(|e| e.as_object())
                            .map(|m| {
                                m.iter()
                                    .filter_map(|(ek, ev)| {
                                        ev.as_str().map(|s| (ek.clone(), s.to_string()))
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        map.insert(
                            k.clone(),
                            BinaryDist {
                                archive: v
                                    .get("archive")
                                    .and_then(|a| a.as_str())
                                    .map(|s| s.to_string()),
                                cmd: cmd.to_string(),
                                args,
                                env,
                                sha256: v
                                    .get("sha256")
                                    .and_then(|s| s.as_str())
                                    .map(|s| s.to_string()),
                            },
                        );
                    }
                }
                if !map.is_empty() {
                    dist.binary = Some(map);
                }
            }
            Some(dist)
        });

        agents.push(RegistryAgent {
            id,
            name,
            version,
            description,
            repository,
            website,
            icon,
            license,
            distribution,
        });
    }
    Ok(RegistryFile {
        version: raw.version,
        agents,
        source: None,
        fetched_at: None,
    })
}

pub fn load_builtin_registry() -> anyhow::Result<RegistryFile> {
    parse_registry(BUILTIN_REGISTRY_JSON, RegistrySource::Builtin)
}

pub fn load_cached_registry() -> Option<RegistryFile> {
    let path = registry_cache_path();
    let data = std::fs::read_to_string(path).ok()?;
    parse_registry(&data, RegistrySource::Cache).ok()
}

/// Load best available registry without network.
pub fn load_registry() -> anyhow::Result<RegistryFile> {
    if let Some(mut cached) = load_cached_registry() {
        cached.source = Some(RegistrySource::Cache);
        normalize_registry(&mut cached);
        return Ok(cached);
    }
    let mut builtin = load_builtin_registry()?;
    normalize_registry(&mut builtin);
    Ok(builtin)
}

/// Fetch live registry and write cache. Falls back to cache/builtin on error.
pub async fn refresh_registry() -> anyhow::Result<RegistryFile> {
    ensure_acp_dirs()?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let resp = client.get(REGISTRY_URL).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("registry HTTP {}", resp.status());
    }
    let text = resp.text().await?;
    let mut file = parse_registry(&text, RegistrySource::Live)?;
    file.fetched_at = Some(chrono::Utc::now().to_rfc3339());
    file.source = Some(RegistrySource::Live);
    // Patch known-bad upstream pins before cache write so re-enable uses fixed launch.
    normalize_registry(&mut file);
    // Cache original text for fidelity + our metadata wrapper
    let cache_body = serde_json::to_string_pretty(&file)?;
    std::fs::write(registry_cache_path(), cache_body)?;
    Ok(file)
}

pub fn find_registry_agent<'a>(
    registry: &'a RegistryFile,
    id: &str,
) -> Option<&'a RegistryAgent> {
    registry.agents.iter().find(|a| a.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_registry_parses() {
        let reg = load_builtin_registry().expect("builtin");
        assert!(!reg.agents.is_empty());
        assert!(reg.agents.iter().any(|a| a.id == "codex-acp"));
    }

    #[test]
    fn resolve_codex_npx() {
        let reg = load_builtin_registry().unwrap();
        let agent = find_registry_agent(&reg, "codex-acp").unwrap();
        let launch = resolve_launch(agent).unwrap();
        assert_eq!(launch.command, "npx");
        assert!(launch.args.iter().any(|a| a.contains("codex-acp")));
    }

    #[test]
    fn normalize_rewrites_missing_grok_1_0_0_pin() {
        let mut reg = load_builtin_registry().unwrap();
        // Simulate broken CDN pin
        let agent = reg.agents.iter_mut().find(|a| a.id == "grok-build").unwrap();
        if let Some(dist) = agent.distribution.as_mut() {
            if let Some(npx) = dist.npx.as_mut() {
                npx.package = "@xai-official/grok@1.0.0".into();
            }
        }
        agent.version = Some("1.0.0".into());
        normalize_registry(&mut reg);
        let agent = find_registry_agent(&reg, "grok-build").unwrap();
        let pkg = agent
            .distribution
            .as_ref()
            .and_then(|d| d.npx.as_ref())
            .map(|n| n.package.as_str())
            .unwrap();
        assert_eq!(pkg, "@xai-official/grok@0.2.121");
        assert_ne!(agent.version.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn resolve_grok_uses_valid_npx_or_local_binary() {
        let mut reg = load_builtin_registry().unwrap();
        normalize_registry(&mut reg);
        let agent = find_registry_agent(&reg, "grok-build").unwrap();
        let launch = resolve_launch(agent).unwrap();
        match launch.kind.as_str() {
            "binary" => {
                // May be basename or absolute well-known path (e.g. ~/.grok/bin/grok).
                assert!(
                    launch.command == "grok" || launch.command.ends_with("/grok") || launch.command.ends_with("\\grok.exe"),
                    "unexpected binary command {}",
                    launch.command
                );
                assert_eq!(launch.args, vec!["agent", "stdio"]);
            }
            "npx" => {
                assert!(
                    launch
                        .args
                        .iter()
                        .any(|a| a.contains("@xai-official/grok@0.2.")),
                    "expected fixed package, got {:?}",
                    launch.args
                );
                assert!(launch.args.iter().any(|a| a == "agent"));
                assert!(launch.args.iter().any(|a| a == "stdio"));
            }
            other => panic!("unexpected launch kind {other}"),
        }
    }
}
