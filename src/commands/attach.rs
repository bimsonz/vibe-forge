use crate::error::VibeError;
use crate::infra::{state::StateManager, tmux::TmuxController};
use std::path::Path;

pub async fn execute(workspace_root: &Path, session_name: Option<String>) -> Result<(), VibeError> {
    let state_manager = StateManager::new(workspace_root);
    let state = state_manager.load().await?;

    let target_session = if let Some(ref name) = session_name {
        state
            .find_session_by_name(name)
            .ok_or_else(|| VibeError::SessionNotFound(name.clone()))?
    } else {
        state
            .active_sessions()
            .into_iter()
            .max_by_key(|s| s.created_at)
            .ok_or_else(|| VibeError::User("No active sessions".into()))?
    };

    let tmux_target = format!("{}:{}", state.tmux_session_name, target_session.name);

    // Select the window first, then attach
    TmuxController::select_window(&tmux_target).await?;
    TmuxController::attach(&state.tmux_session_name).await?;

    Ok(())
}
