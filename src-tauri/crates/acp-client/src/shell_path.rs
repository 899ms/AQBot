//! Resolve the current user's login-shell PATH for GUI-launched processes.

use std::collections::HashMap;
#[cfg(unix)]
use std::collections::HashSet;
use std::sync::OnceLock;

pub(crate) fn get_shell_path() -> &'static str {
    static SHELL_PATH: OnceLock<String> = OnceLock::new();
    SHELL_PATH.get_or_init(|| resolve_login_shell_path().unwrap_or_default())
}

pub(crate) fn inject_shell_path(env: &mut HashMap<String, String>, shell_path: &str) {
    if !shell_path.is_empty() && !has_path_override(env) {
        env.insert("PATH".into(), shell_path.into());
    }
}

#[cfg(windows)]
fn has_path_override(env: &HashMap<String, String>) -> bool {
    env.keys().any(|key| key.eq_ignore_ascii_case("PATH"))
}

#[cfg(not(windows))]
fn has_path_override(env: &HashMap<String, String>) -> bool {
    env.contains_key("PATH")
}

#[cfg(unix)]
fn resolve_login_shell_path() -> Option<String> {
    let current_path = std::env::var("PATH").ok();
    let mut best_path: Option<String> = None;

    for shell in shell_candidates() {
        if let Some(candidate_path) = read_path_from_shell(&shell) {
            let merged = merge_paths(&candidate_path, current_path.as_deref());
            if path_score(&merged) > best_path.as_ref().map(|path| path_score(path)).unwrap_or(0) {
                best_path = Some(merged);
            }
        }
    }

    best_path.or(current_path)
}

#[cfg(not(unix))]
fn resolve_login_shell_path() -> Option<String> {
    std::env::var("PATH").ok()
}

#[cfg(unix)]
fn shell_candidates() -> Vec<String> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    for candidate in [
        std::env::var("SHELL").ok(),
        Some("zsh".into()),
        Some("/bin/zsh".into()),
        Some("bash".into()),
        Some("/bin/bash".into()),
        Some("sh".into()),
        Some("/bin/sh".into()),
    ]
    .into_iter()
    .flatten()
    {
        if !candidate.is_empty() && seen.insert(candidate.clone()) {
            candidates.push(candidate);
        }
    }

    candidates
}

#[cfg(unix)]
fn read_path_from_shell(shell: &str) -> Option<String> {
    const START: &str = "__AQBOT_PATH_START__";
    const END: &str = "__AQBOT_PATH_END__";
    let print_path = format!("printf '{START}'; printenv PATH; printf '{END}'");
    let output = std::process::Command::new(shell)
        .args(["-i", "-l", "-c", &print_path])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    extract_marked_path(&output.stdout, START, END)
}

#[cfg(unix)]
fn extract_marked_path(output: &[u8], start: &str, end: &str) -> Option<String> {
    let stdout = String::from_utf8(output.to_vec()).ok()?;
    let start_index = stdout.find(start)? + start.len();
    let end_index = stdout[start_index..].find(end)? + start_index;
    let path = stdout[start_index..end_index].trim().to_string();
    (!path.is_empty()).then_some(path)
}

#[cfg(unix)]
fn merge_paths(primary: &str, fallback: Option<&str>) -> String {
    let mut merged = Vec::new();
    let mut seen = HashSet::new();

    for path_list in [Some(primary), fallback] {
        for segment in path_list
            .unwrap_or_default()
            .split(':')
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
        {
            if seen.insert(segment.to_string()) {
                merged.push(segment.to_string());
            }
        }
    }

    merged.join(":")
}

#[cfg(unix)]
fn path_score(path: &str) -> usize {
    path.split(':')
        .filter(|segment| !segment.is_empty())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_path_is_injected_only_when_the_agent_does_not_override_it() {
        let mut generated = HashMap::new();
        inject_shell_path(&mut generated, "/current-user/bin:/usr/bin");
        assert_eq!(
            generated.get("PATH").map(String::as_str),
            Some("/current-user/bin:/usr/bin")
        );

        let mut custom = HashMap::from([("PATH".into(), "/custom/bin".into())]);
        inject_shell_path(&mut custom, "/current-user/bin:/usr/bin");
        assert_eq!(custom.len(), 1);
        assert_eq!(custom.get("PATH").map(String::as_str), Some("/custom/bin"));
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_path_override_is_case_sensitive() {
        let mut env = HashMap::from([("Path".into(), "/not-the-unix-path".into())]);
        inject_shell_path(&mut env, "/current-user/bin:/usr/bin");

        assert_eq!(
            env.get("Path").map(String::as_str),
            Some("/not-the-unix-path")
        );
        assert_eq!(
            env.get("PATH").map(String::as_str),
            Some("/current-user/bin:/usr/bin")
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_path_override_is_case_insensitive() {
        let mut env = HashMap::from([("Path".into(), r"C:\custom\bin".into())]);
        inject_shell_path(&mut env, r"C:\current-user\bin");

        assert_eq!(env.len(), 1);
        assert_eq!(env.get("Path").map(String::as_str), Some(r"C:\custom\bin"));
    }

    #[cfg(unix)]
    #[test]
    fn marked_path_ignores_interactive_shell_noise() {
        let output = b"noise\n__AQBOT_PATH_START__/opt/bin:/usr/bin__AQBOT_PATH_END__\n";
        let path =
            extract_marked_path(output, "__AQBOT_PATH_START__", "__AQBOT_PATH_END__").unwrap();
        assert_eq!(path, "/opt/bin:/usr/bin");
    }

    #[cfg(unix)]
    #[test]
    fn merged_path_preserves_order_and_deduplicates_segments() {
        assert_eq!(
            merge_paths("/opt/bin:/usr/bin", Some("/usr/bin:/bin")),
            "/opt/bin:/usr/bin:/bin"
        );
    }
}
