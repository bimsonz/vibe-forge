use crate::domain::workspace::WorkspaceKind;
use crate::error::VibeError;
use crate::infra::{git, state::StateManager};
use std::path::Path;

pub async fn execute(workspace_root: &Path) -> Result<(), VibeError> {
    let state_manager = StateManager::new(workspace_root);
    let mut state = state_manager.load().await?;

    if state.workspace.kind != WorkspaceKind::MultiRepo {
        println!("Not a multi-repo workspace. Nothing to refresh.");
        return Ok(());
    }

    let current_repos = git::discover_repos(workspace_root)?;
    let old_names: Vec<String> = state.workspace.repos.iter().map(|r| r.name.clone()).collect();
    let new_names: Vec<String> = current_repos.iter().map(|r| r.name.clone()).collect();

    // Find newly added repos
    let added: Vec<&str> = new_names
        .iter()
        .filter(|n| !old_names.contains(n))
        .map(|s| s.as_str())
        .collect();

    // Find removed repos
    let removed: Vec<&str> = old_names
        .iter()
        .filter(|o| !new_names.contains(o))
        .map(|s| s.as_str())
        .collect();

    if added.is_empty() && removed.is_empty() {
        println!("No changes. {} repos tracked.", state.workspace.repos.len());
        return Ok(());
    }

    if !added.is_empty() {
        println!("Added: {}", added.join(", "));
    }
    if !removed.is_empty() {
        println!("Removed: {}", removed.join(", "));
    }

    state.workspace.repos = current_repos;
    state_manager.save(&state).await?;

    println!(
        "Now tracking {} repos: {}",
        new_names.len(),
        new_names.join(", ")
    );
    println!("Note: existing sessions are not affected. Only new sessions will include the updated repo list.");

    Ok(())
}
