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
            if session.repo_worktrees.is_empty() {
                // Single-repo: remove the single worktree
                if session.worktree_path.exists() {
                    match git::remove_worktree(workspace_root, &session.worktree_path, false).await {
                        Ok(()) => println!("    Worktree removed"),
                        Err(e) => println!("    Warning: {e}"),
                    }
                }
            } else {
                // Multi-repo: remove each repo's worktree
                for (repo_name, wt_path) in &session.repo_worktrees {
                    if wt_path.exists() {
                        // Find the repo root
                        let repo_root = state
                            .workspace
                            .repos
                            .iter()
                            .find(|r| r.name == *repo_name)
                            .map(|r| r.root.clone());
                        if let Some(repo_root) = repo_root {
                            match git::remove_worktree(&repo_root, wt_path, false).await {
                                Ok(()) => println!("    Worktree removed: {repo_name}"),
                                Err(e) => println!("    Warning ({repo_name}): {e}"),
                            }
                        }
                    }
                }
                // Remove session root directory
                if session.worktree_path.exists() {
                    let _ = tokio::fs::remove_dir_all(&session.worktree_path).await;
                    println!("    Session root removed");
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
        if state.workspace.repos.is_empty() {
            git::prune(workspace_root).await?;
        } else {
            for repo in &state.workspace.repos {
                git::prune(&repo.root).await?;
            }
        }

        println!("Cleanup complete.");
    } else {
        println!("\nDry run — no changes made. Remove --dry-run to execute.");
    }

    Ok(())
}
