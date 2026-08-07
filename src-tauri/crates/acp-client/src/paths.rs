use std::path::PathBuf;

/// Config/cache root: `~/.aqbot/acp/`
pub fn acp_home() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".aqbot")
        .join("acp")
}

pub fn agents_toml_path() -> PathBuf {
    acp_home().join("agents.toml")
}

pub fn registry_cache_path() -> PathBuf {
    acp_home().join("registry.cache.json")
}

pub fn ensure_acp_dirs() -> std::io::Result<()> {
    std::fs::create_dir_all(acp_home())
}
