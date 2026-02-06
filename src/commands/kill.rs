use crate::config::MergedConfig;
use crate::error::ForgeError;
use crate::infra::{git, state::StateManager, tmux::TmuxController};
use std::path::Path;
use tracing::{info, warn};

pub async fn execute(
    workspace_root: &Path,
    target: String,
    force: bool,
    delete_branch: bool,
    _config: &MergedConfig,
) -> Result<(), ForgeError> {
    let state_manager = StateManager::new(workspace_root);
    let mut state = state_manager.load().await?;

    let session = state
        .find_session_by_name_mut(&target)
        .ok_or_else(|| ForgeError::SessionNotFound(target.clone()))?;

    if session.is_main {
        return Err(ForgeError::User(
            "Cannot kill the main session".into(),
        ));
    }

    if !force && session.is_active() {
        warn!(session = %target, "session is active — use --force to kill it");
        return Ok(());
    }

    let worktree_path = session.worktree_path.clone();
    let session_name = session.name.clone();
    let tmux_window = format!("{}:{}", state.tmux_session_name, session_name);

    info!(session = %session_name, "killing session");

    // Kill tmux window (best effort)
    let _ = TmuxController::kill_window(&tmux_window).await;
    info!(session = %session_name, "tmux window removed");

    // Remove git worktree
    if worktree_path.exists() {
        match git::remove_worktree(workspace_root, &worktree_path, delete_branch).await {
            Ok(()) => info!(path = %worktree_path.display(), "worktree removed"),
            Err(e) => warn!(error = %e, "failed to remove worktree"),
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
