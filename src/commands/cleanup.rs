use crate::domain::session::SessionStatus;
use crate::error::ForgeError;
use crate::infra::{git, state::StateManager};
use std::path::Path;

pub async fn execute(
    workspace_root: &Path,
    all: bool,
    dry_run: bool,
) -> Result<(), ForgeError> {
    let state_manager = StateManager::new(workspace_root);
    let mut state = state_manager.load().await?;

    // Find archived or completed sessions
    let to_clean: Vec<_> = state
        .sessions
        .iter()
        .filter(|s| {
            if all {
                matches!(
                    s.status,
                    SessionStatus::Archived | SessionStatus::Completed
                )
            } else {
                matches!(s.status, SessionStatus::Archived)
            }
        })
        .cloned()
        .collect();

    if to_clean.is_empty() {
        println!("Nothing to clean up.");
        return Ok(());
    }

    println!(
        "{} {} session(s) to clean up:",
        if dry_run { "Would remove" } else { "Removing" },
        to_clean.len()
    );

    for session in &to_clean {
        println!("  {} [{}]", session.name, session.status);

        if !dry_run {
            // Remove worktree if it still exists
            if session.worktree_path.exists() {
                match git::remove_worktree(workspace_root, &session.worktree_path, false).await {
                    Ok(()) => println!("    Worktree removed"),
                    Err(e) => println!("    Warning: {e}"),
                }
            }
        }
    }

    if !dry_run {
        // Remove cleaned sessions from state
        let clean_ids: Vec<_> = to_clean.iter().map(|s| s.id).collect();
        state.sessions.retain(|s| !clean_ids.contains(&s.id));
        state
            .agents
            .retain(|a| !clean_ids.contains(&a.parent_session));
        state_manager.save(&state).await?;

        // Prune git worktree references
        git::prune(workspace_root).await?;

        println!("Cleanup complete.");
    } else {
        println!("\nDry run — no changes made. Remove --dry-run to execute.");
    }

    Ok(())
}
