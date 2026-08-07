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

/// Resolve a launch command for the current platform.
/// Prefer npx/uvx (no download); fall back to binary cmd name only (V1 does not install).
pub fn resolve_launch(agent: &RegistryAgent) -> Option<ResolvedLaunch> {
    let dist = agent.distribution.as_ref()?;

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
        return Ok(cached);
    }
    load_builtin_registry()
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
}
