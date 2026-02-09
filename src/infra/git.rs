use crate::domain::workspace::RepoInfo;
use crate::error::VibeError;
use git2::Repository;
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tracing;

#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: String,
}

/// Detect the repository root from any path within it
pub fn find_repo_root(start_path: &Path) -> Result<PathBuf, VibeError> {
    let repo = Repository::discover(start_path).map_err(|_| VibeError::NotGitRepo)?;
    let workdir = repo.workdir().ok_or(VibeError::Git(
        "Bare repositories are not supported".into(),
    ))?;
    Ok(workdir.to_path_buf())
}

/// Get the default branch (main or master)
pub fn default_branch(repo_root: &Path) -> Result<String, VibeError> {
    let repo = Repository::open(repo_root).map_err(|_| VibeError::NotGitRepo)?;
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
/// Fetches origin and updates the default branch first so the new worktree
/// starts from the latest code. Naming convention: {repo_name}-vibe-{short_id}
pub async fn create_worktree(
    repo_root: &Path,
    branch_name: &str,
    base_ref: Option<&str>,
    worktree_base_dir: &Path,
) -> Result<WorktreeInfo, VibeError> {
    // Fetch origin so we have the latest refs
    let fetch_output = Command::new("git")
        .current_dir(repo_root)
        .args(["fetch", "origin"])
        .output()
        .await?;
    if !fetch_output.status.success() {
        tracing::warn!(
            stderr = %String::from_utf8_lossy(&fetch_output.stderr),
            "git fetch origin failed, continuing with local state"
        );
    }

    // If no explicit base, use origin's default branch to ensure we're up to date
    let resolved_base = if let Some(b) = base_ref {
        b.to_string()
    } else {
        let default = default_branch(repo_root)?;
        format!("origin/{default}")
    };

    let short_id = &uuid::Uuid::new_v4().to_string()[..8];
    let repo_name = repo_root
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let worktree_dir_name = format!("{repo_name}-vibe-{short_id}");
    let worktree_path = worktree_base_dir.join(&worktree_dir_name);

    let base = &resolved_base;

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
            return Err(VibeError::Git(
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
) -> Result<(), VibeError> {
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
        return Err(VibeError::Git(
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

/// List all worktrees managed by vibe (identified by naming convention)
pub async fn list_vibe_worktrees(repo_root: &Path) -> Result<Vec<WorktreeInfo>, VibeError> {
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
                .is_some_and(|n| n.to_string_lossy().contains("-vibe-"))
            {
                worktrees.push(WorktreeInfo { path, branch });
            }
        }
    }

    Ok(worktrees)
}

/// Prune stale worktree references
pub async fn prune(repo_root: &Path) -> Result<(), VibeError> {
    Command::new("git")
        .current_dir(repo_root)
        .args(["worktree", "prune"])
        .output()
        .await?;
    Ok(())
}

/// Scan immediate subdirectories of `parent_dir` for git repositories.
/// Returns a `RepoInfo` for each subdirectory that contains a `.git` directory or file.
pub fn discover_repos(parent_dir: &Path) -> Result<Vec<RepoInfo>, VibeError> {
    let mut repos = Vec::new();
    let entries = std::fs::read_dir(parent_dir)
        .map_err(|e| VibeError::Git(format!("Cannot read directory: {e}")))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // Skip hidden directories (e.g. .vibe, .git)
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        {
            continue;
        }
        // Check if this subdir is a git repo
        if Repository::open(&path).is_ok() {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let db = default_branch(&path).unwrap_or_else(|_| "main".into());
            let url = remote_url(&path);
            repos.push(RepoInfo {
                root: path,
                name,
                default_branch: db,
                remote_url: url,
            });
        }
    }

    repos.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(repos)
}

/// Create a worktree at an exact path (used for multi-repo sessions).
///
/// Unlike `create_worktree`, the caller specifies the exact target path rather
/// than having one generated. Fetches origin first, just like `create_worktree`.
pub async fn create_worktree_at(
    repo_root: &Path,
    branch_name: &str,
    base_ref: Option<&str>,
    exact_path: &Path,
) -> Result<WorktreeInfo, VibeError> {
    // Fetch origin
    let fetch_output = Command::new("git")
        .current_dir(repo_root)
        .args(["fetch", "origin"])
        .output()
        .await?;
    if !fetch_output.status.success() {
        tracing::warn!(
            repo = %repo_root.display(),
            stderr = %String::from_utf8_lossy(&fetch_output.stderr),
            "git fetch origin failed, continuing with local state"
        );
    }

    let resolved_base = if let Some(b) = base_ref {
        b.to_string()
    } else {
        let default = default_branch(repo_root)?;
        format!("origin/{default}")
    };

    // Try creating with -b (new branch)
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["worktree", "add", "-b", branch_name])
        .arg(exact_path)
        .arg(&resolved_base)
        .output()
        .await?;

    if !output.status.success() {
        // Branch might already exist — try without -b
        let output2 = Command::new("git")
            .current_dir(repo_root)
            .args(["worktree", "add"])
            .arg(exact_path)
            .arg(branch_name)
            .output()
            .await?;

        if !output2.status.success() {
            return Err(VibeError::Git(
                String::from_utf8_lossy(&output2.stderr).to_string(),
            ));
        }
    }

    Ok(WorktreeInfo {
        path: exact_path.to_path_buf(),
        branch: branch_name.to_string(),
    })
}

async fn worktree_branch(worktree_path: &Path) -> Result<String, VibeError> {
    let output = Command::new("git")
        .current_dir(worktree_path)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .await?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
