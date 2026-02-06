use crate::domain::session::SessionStatus;
use crate::error::ForgeError;
use crate::infra::{claude, gh, state::StateManager, tmux::TmuxController};
use std::path::Path;

pub async fn execute(workspace_root: &Path) -> Result<(), ForgeError> {
    println!("vibe doctor: checking workspace health\n");
    let mut issues = 0;
    let mut fixed = 0;

    // 1. Check tools
    print!("  tmux: ");
    if TmuxController::is_available() {
        println!("ok");
    } else {
        println!("NOT FOUND - install with: brew install tmux");
        issues += 1;
    }

    print!("  claude: ");
    if claude::is_available() {
        println!("ok");
    } else {
        println!("NOT FOUND - install from: https://claude.ai/code");
        issues += 1;
    }

    print!("  gh: ");
    if gh::is_available() {
        println!("ok");
    } else {
        println!("NOT FOUND - install from: https://cli.github.com (needed for PR review)");
        issues += 1;
    }

    // 2. Check forge state
    let state_manager = StateManager::new(workspace_root);
    if !state_manager.is_initialized() {
        println!("\n  State: NOT INITIALIZED - run `vibe init`");
        issues += 1;
        println!("\n{issues} issue(s) found.");
        return Ok(());
    }
    println!("  state: ok");

    let mut state = state_manager.load().await?;

    // 3. Check tmux session
    let tmux_exists = TmuxController::session_exists(&state.tmux_session_name)
        .await
        .unwrap_or(false);
    print!("  tmux session '{}': ", state.tmux_session_name);
    if tmux_exists {
        println!("running");
    } else {
        println!("not running");
    }

    // 4. Reconcile sessions with actual state
    println!("\n  Reconciling sessions...");
    for session in &mut state.sessions {
        if matches!(session.status, SessionStatus::Archived) {
            continue;
        }

        let worktree_exists = session.worktree_path.exists();
        let tmux_window_exists = if tmux_exists {
            let target = format!("{}:{}", state.tmux_session_name, session.name);
            TmuxController::select_window(&target).await.is_ok()
        } else {
            false
        };

        if !worktree_exists && !tmux_window_exists {
            println!(
                "    {} - worktree gone, tmux gone -> marking archived",
                session.name
            );
            session.status = SessionStatus::Archived;
            fixed += 1;
        } else if !worktree_exists {
            println!(
                "    {} - worktree gone but tmux alive -> marking failed",
                session.name
            );
            session.status = SessionStatus::Failed("Worktree missing".into());
            fixed += 1;
        } else if !tmux_window_exists && matches!(session.status, SessionStatus::Active) {
            println!(
                "    {} - tmux gone but worktree exists -> marking paused",
                session.name
            );
            session.status = SessionStatus::Paused;
            fixed += 1;
        } else {
            println!("    {} - ok", session.name);
        }
    }

    // 5. Check for orphaned worktrees (forge-managed worktrees not in state)
    println!("\n  Checking for orphaned worktrees...");
    let worktrees = crate::infra::git::list_forge_worktrees(workspace_root).await?;
    let known_paths: Vec<_> = state
        .sessions
        .iter()
        .map(|s| s.worktree_path.clone())
        .collect();

    for wt in &worktrees {
        if !known_paths.contains(&wt.path) {
            println!(
                "    ORPHAN: {} (branch: {})",
                wt.path.display(),
                wt.branch
            );
            println!("      Remove with: git worktree remove --force {}", wt.path.display());
            issues += 1;
        }
    }

    // Save reconciled state
    if fixed > 0 {
        state_manager.save(&state).await?;
        println!("\n  Fixed {fixed} session(s).");
    }

    if issues == 0 && fixed == 0 {
        println!("\nAll clear.");
    } else {
        println!("\n{issues} issue(s) found, {fixed} auto-fixed.");
    }

    Ok(())
}
