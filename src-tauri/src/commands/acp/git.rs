// ACP project Git inspection and checkout commands.

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
