use crate::config::MergedConfig;
use crate::error::VibeError;
use crate::infra::{git, state::StateManager, tmux::TmuxController};
use std::path::Path;
use tracing::{info, warn};

pub async fn execute(
    workspace_root: &Path,
    target: String,
    force: bool,
    delete_branch: bool,
    _config: &MergedConfig,
) -> Result<(), VibeError> {
    let state_manager = StateManager::new(workspace_root);
    let mut state = state_manager.load().await?;

    let session = state
        .find_session_by_name_mut(&target)
        .ok_or_else(|| VibeError::SessionNotFound(target.clone()))?;

    if session.is_main {
        return Err(VibeError::User(
            "Cannot kill the main session".into(),
        ));
    }

    if !force && session.is_active() {
        warn!(session = %target, "session is active — use --force to kill it");
        return Ok(());
    }

    let worktree_path = session.worktree_path.clone();
    let repo_worktrees = session.repo_worktrees.clone();
    let session_name = session.name.clone();
    let tmux_window = format!("{}:{}", state.tmux_session_name, session_name);

    info!(session = %session_name, "killing session");

    // Kill tmux window (best effort)
    let _ = TmuxController::kill_window(&tmux_window).await;
    info!(session = %session_name, "tmux window removed");

    if repo_worktrees.is_empty() {
        // Single-repo mode: remove the single worktree
        if worktree_path.exists() {
            match git::remove_worktree(workspace_root, &worktree_path, delete_branch).await {
                Ok(()) => info!(path = %worktree_path.display(), "worktree removed"),
                Err(e) => warn!(error = %e, "failed to remove worktree"),
            }
        }
    } else {
        // Multi-repo mode: remove worktree from each repo
        for (repo_name, wt_path) in &repo_worktrees {
            // Find the original repo root from workspace.repos
            let repo_root = state
                .workspace
                .repos
                .iter()
                .find(|r| r.name == *repo_name)
                .map(|r| r.root.clone());

            if let Some(repo_root) = repo_root {
                if wt_path.exists() {
                    match git::remove_worktree(&repo_root, wt_path, delete_branch).await {
                        Ok(()) => info!(repo = %repo_name, path = %wt_path.display(), "repo worktree removed"),
                        Err(e) => warn!(repo = %repo_name, error = %e, "failed to remove repo worktree"),
                    }
                }
            } else {
                warn!(repo = %repo_name, "repo not found in workspace, skipping worktree removal");
            }
        }

        // Remove the session root directory
        if worktree_path.exists() {
            match tokio::fs::remove_dir_all(&worktree_path).await {
                Ok(()) => info!(path = %worktree_path.display(), "session root removed"),
                Err(e) => warn!(error = %e, "failed to remove session root directory"),
            }
        }
    }

    // Remove session and associated agents from state
    let session_id = state
        .find_session_by_name(&target)
        .map(|s| s.id)
        .unwrap();
    state.agents.retain(|a| a.parent_session != session_id);
    state.sessions.retain(|s| s.id != session_id);

    state_manager.save(&state).await?;

    info!(session = %session_name, "session killed and removed");
    Ok(())
}
