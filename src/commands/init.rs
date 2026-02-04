use crate::config::{self, load_config};
use crate::domain::workspace::{Workspace, WorkspaceState};
use crate::error::ForgeError;
use crate::infra::{git, state::StateManager};
use std::path::Path;

pub async fn execute(workspace_root: &Path) -> Result<(), ForgeError> {
    let state_manager = StateManager::new(workspace_root);

    if state_manager.is_initialized() {
        println!("Forge is already initialized in this workspace.");
        return Ok(());
    }

    // Ensure global config dir exists
    config::ensure_global_config_dir()?;

    // Detect git info
    let default_branch = git::default_branch(workspace_root)?;
    let remote_url = git::remote_url(workspace_root);
    let repo_name = workspace_root
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let config = load_config(Some(workspace_root))?;

    let worktree_base_dir = config.worktree_base_dir(workspace_root);
    let tmux_session_name = format!("{}{}", config.tmux_session_prefix(), repo_name);

    let workspace = Workspace {
        root: workspace_root.to_path_buf(),
        name: repo_name,
        default_branch,
        remote_url,
        worktree_prefix: config.global.worktree_suffix.clone(),
        worktree_base_dir,
    };

    let state = WorkspaceState::new(workspace, tmux_session_name);

    // Create directory structure
    state_manager.init().await?;

    // Save initial state
    state_manager.save(&state).await?;

    println!("Forge initialized in {}", workspace_root.display());
    println!("  .forge/ directory created");
    println!("  Run `forge new <name>` to create your first session");

    Ok(())
}
