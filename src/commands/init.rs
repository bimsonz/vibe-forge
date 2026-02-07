use crate::config::{self, load_config};
use crate::domain::workspace::{Workspace, WorkspaceKind, WorkspaceState};
use crate::error::ForgeError;
use crate::infra::{git, state::StateManager};
use std::path::Path;

pub async fn execute(workspace_root: &Path) -> Result<(), ForgeError> {
    let state_manager = StateManager::new(workspace_root);

    if state_manager.is_initialized() {
        println!("Vibe is already initialized in this workspace.");
        return Ok(());
    }

    // Ensure global config dir exists
    config::ensure_global_config_dir()?;

    // Determine if this is a single-repo or multi-repo workspace
    let is_git_repo = git::find_repo_root(workspace_root).is_ok();

    if is_git_repo {
        init_single_repo(workspace_root, &state_manager).await
    } else {
        init_multi_repo(workspace_root, &state_manager).await
    }
}

async fn init_single_repo(
    workspace_root: &Path,
    state_manager: &StateManager,
) -> Result<(), ForgeError> {
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
        kind: WorkspaceKind::SingleRepo,
        repos: vec![],
    };

    let state = WorkspaceState::new(workspace, tmux_session_name);
    state_manager.init().await?;
    state_manager.save(&state).await?;

    println!("Vibe initialized in {}", workspace_root.display());
    println!("  .vibe/ directory created");
    println!("  Run `vibe new <name>` to create your first session");

    Ok(())
}

async fn init_multi_repo(
    workspace_root: &Path,
    state_manager: &StateManager,
) -> Result<(), ForgeError> {
    let repos = git::discover_repos(workspace_root)?;
    if repos.is_empty() {
        return Err(ForgeError::NoReposFound);
    }

    let repo_names: Vec<String> = repos.iter().map(|r| r.name.clone()).collect();
    let dir_name = workspace_root
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let config = load_config(Some(workspace_root))?;
    let worktree_base_dir = config.worktree_base_dir(workspace_root);
    let tmux_session_name = format!("{}{}", config.tmux_session_prefix(), dir_name);

    // Use "main" as default branch — individual repos track their own defaults in RepoInfo
    let workspace = Workspace {
        root: workspace_root.to_path_buf(),
        name: dir_name,
        default_branch: "main".into(),
        remote_url: None,
        worktree_prefix: config.global.worktree_suffix.clone(),
        worktree_base_dir,
        kind: WorkspaceKind::MultiRepo,
        repos,
    };

    let state = WorkspaceState::new(workspace, tmux_session_name);
    state_manager.init().await?;
    state_manager.save(&state).await?;

    println!(
        "Vibe initialized (multi-repo) in {}",
        workspace_root.display()
    );
    println!(
        "  Discovered {} repositories: {}",
        repo_names.len(),
        repo_names.join(", ").as_str()
    );
    println!("  .vibe/ directory created");
    println!("  Run `vibe new <name>` to create your first session");

    Ok(())
}
