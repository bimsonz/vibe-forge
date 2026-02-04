use crate::config::MergedConfig;
use crate::domain::session::SessionStatus;
use crate::error::ForgeError;
use crate::infra::{git, state::StateManager, tmux::TmuxController};
use std::path::Path;

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

    if !force && session.is_active() {
        println!(
            "Session '{}' is active. Use --force to kill it anyway.",
            target
        );
        return Ok(());
    }

    let worktree_path = session.worktree_path.clone();
    let session_name = session.name.clone();
    let tmux_window = format!("{}:{}", state.tmux_session_name, session_name);

    println!("Killing session '{session_name}'...");

    // Kill tmux window (best effort)
    let _ = TmuxController::kill_window(&tmux_window).await;
    println!("  tmux window removed");

    // Remove git worktree
    if worktree_path.exists() {
        match git::remove_worktree(workspace_root, &worktree_path, delete_branch).await {
            Ok(()) => println!("  Worktree removed: {}", worktree_path.display()),
            Err(e) => println!("  Warning: Failed to remove worktree: {e}"),
        }
    }

    // Update session status
    let session = state.find_session_by_name_mut(&target).unwrap();
    session.status = SessionStatus::Archived;

    // Remove associated agents
    let session_id = session.id;
    state.agents.retain(|a| a.parent_session != session_id);

    state_manager.save(&state).await?;

    println!("Session '{session_name}' killed and archived.");
    Ok(())
}
