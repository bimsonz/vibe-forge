use crate::error::ForgeError;
use git2::Repository;
use std::path::{Path, PathBuf};
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: String,
}

/// Detect the repository root from any path within it
pub fn find_repo_root(start_path: &Path) -> Result<PathBuf, ForgeError> {
    let repo = Repository::discover(start_path).map_err(|_| ForgeError::NotGitRepo)?;
    let workdir = repo.workdir().ok_or(ForgeError::Git(
        "Bare repositories are not supported".into(),
    ))?;
    Ok(workdir.to_path_buf())
}

/// Get the default branch (main or master)
pub fn default_branch(repo_root: &Path) -> Result<String, ForgeError> {
    let repo = Repository::open(repo_root).map_err(|_| ForgeError::NotGitRepo)?;
    for candidate in &["refs/remotes/origin/main", "refs/remotes/origin/master"] {
        if repo.find_reference(candidate).is_ok() {
            return Ok(candidate.rsplit('/').next().unwrap().to_string());
        }
    }
    Ok("main".to_string())
}

/// Get the remote URL for the repo
pub fn remote_url(repo_root: &Path) -> Option<String> {
    let repo = Repository::open(repo_root).ok()?;
    let remote = repo.find_remote("origin").ok()?;
    remote.url().map(|s| s.to_string())
}

/// Create a worktree for a new session.
///
/// Naming convention: {repo_name}-forge-{short_id}
pub async fn create_worktree(
    repo_root: &Path,
    branch_name: &str,
    base_ref: Option<&str>,
    worktree_base_dir: &Path,
) -> Result<WorktreeInfo, ForgeError> {
    let short_id = &uuid::Uuid::new_v4().to_string()[..8];
    let repo_name = repo_root
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let worktree_dir_name = format!("{repo_name}-forge-{short_id}");
    let worktree_path = worktree_base_dir.join(&worktree_dir_name);

    let base = base_ref.unwrap_or("HEAD");

    // Try creating with -b (new branch)
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["worktree", "add", "-b", branch_name])
        .arg(&worktree_path)
        .arg(base)
        .output()
        .await?;

    if !output.status.success() {
        // Branch might already exist — try without -b
        let output2 = Command::new("git")
            .current_dir(repo_root)
            .args(["worktree", "add"])
            .arg(&worktree_path)
            .arg(branch_name)
            .output()
            .await?;

        if !output2.status.success() {
            return Err(ForgeError::Git(
                String::from_utf8_lossy(&output2.stderr).to_string(),
            ));
        }
    }

    Ok(WorktreeInfo {
        path: worktree_path,
        branch: branch_name.to_string(),
    })
}

/// Remove a worktree and optionally delete the branch
pub async fn remove_worktree(
    repo_root: &Path,
    worktree_path: &Path,
    delete_branch: bool,
) -> Result<(), ForgeError> {
    // Get the branch name before removing
    let branch = if delete_branch {
        worktree_branch(worktree_path).await.ok()
    } else {
        None
    };

    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["worktree", "remove", "--force"])
        .arg(worktree_path)
        .output()
        .await?;

    if !output.status.success() {
        return Err(ForgeError::Git(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    if let Some(branch) = branch {
        // Best-effort branch deletion
        let _ = Command::new("git")
            .current_dir(repo_root)
            .args(["branch", "-D", &branch])
            .output()
            .await;
    }

    Ok(())
}

/// List all worktrees managed by forge (identified by naming convention)
pub async fn list_forge_worktrees(repo_root: &Path) -> Result<Vec<WorktreeInfo>, ForgeError> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut worktrees = Vec::new();

    for block in stdout.split("\n\n") {
        let mut path = None;
        let mut branch = None;
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(p));
            }
            if let Some(b) = line.strip_prefix("branch refs/heads/") {
                branch = Some(b.to_string());
            }
        }
        if let (Some(path), Some(branch)) = (path, branch) {
            if path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().contains("-forge-"))
            {
                worktrees.push(WorktreeInfo { path, branch });
            }
        }
    }

    Ok(worktrees)
}

/// Prune stale worktree references
pub async fn prune(repo_root: &Path) -> Result<(), ForgeError> {
    Command::new("git")
        .current_dir(repo_root)
        .args(["worktree", "prune"])
        .output()
        .await?;
    Ok(())
}

async fn worktree_branch(worktree_path: &Path) -> Result<String, ForgeError> {
    let output = Command::new("git")
        .current_dir(worktree_path)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .await?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
