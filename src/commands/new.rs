use crate::config::MergedConfig;
use crate::domain::session::{Session, SessionStatus};
use crate::domain::template::AgentTemplate;
use crate::domain::workspace::WorkspaceKind;
use crate::error::ForgeError;
use crate::infra::{claude, git, state::StateManager, tmux::TmuxController};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

pub async fn execute(
    workspace_root: &Path,
    name: String,
    branch: Option<String>,
    base: Option<String>,
    template_name: Option<String>,
    system_prompt: Option<String>,
    headless: bool,
    prompt: Option<String>,
    config: &MergedConfig,
) -> Result<(), ForgeError> {
    let state_manager = StateManager::new(workspace_root);
    let mut state = state_manager.load().await?;

    // Check for duplicate session name
    if state.find_session_by_name(&name).is_some() {
        return Err(ForgeError::User(format!(
            "Session '{name}' already exists. Use a different name."
        )));
    }

    let branch_name = branch.unwrap_or_else(|| format!("feat/{name}"));
    let base_ref = base.as_deref();

    info!(session = %name, "creating session");

    let (session_worktree_path, repo_worktrees) = match state.workspace.kind {
        WorkspaceKind::SingleRepo => {
            let worktree_base_dir = config.worktree_base_dir(workspace_root);
            let worktree = git::create_worktree(
                workspace_root,
                &branch_name,
                base_ref,
                &worktree_base_dir,
            )
            .await?;
            info!(worktree = %worktree.path.display(), "worktree created");
            (worktree.path, BTreeMap::new())
        }
        WorkspaceKind::MultiRepo => {
            create_multi_repo_worktrees(
                &state.workspace.repos,
                &branch_name,
                base_ref,
                &config.worktree_base_dir(workspace_root),
                &state.workspace.name,
            )
            .await?
        }
    };

    // Ensure tmux session exists
    TmuxController::ensure_session(&state.tmux_session_name).await?;

    // Create tmux window
    let window_id = TmuxController::create_window(
        &state.tmux_session_name,
        &name,
        session_worktree_path.to_str().unwrap_or("."),
    )
    .await?;
    info!(window = %name, "tmux window created");

    // Lock the window name so tmux doesn't auto-rename it when Claude starts
    let window_target = format!("{}:{}", state.tmux_session_name, name);
    let _ = TmuxController::disable_auto_rename_for(&window_target).await;

    // Create session record
    let mut session = Session::new(
        name.clone(),
        branch_name,
        session_worktree_path,
        window_id.clone(),
    );
    session.repo_worktrees = repo_worktrees;

    // Load template if specified
    let resolved_system_prompt = if let Some(sp) = system_prompt {
        Some(sp)
    } else if let Some(ref tmpl_name) = template_name {
        let template_dirs = config.template_dirs(workspace_root);
        let template = AgentTemplate::load(tmpl_name, &template_dirs)?;
        session.template = Some(tmpl_name.clone());
        Some(template.system_prompt)
    } else {
        None
    };

    if headless {
        let task_prompt = prompt.ok_or_else(|| {
            ForgeError::User("--prompt is required in headless mode".into())
        })?;

        // Run headless claude
        let tmux_target = format!("{}:{}", state.tmux_session_name, name);
        let cmd = format!(
            "claude -p --output-format json {} '{}'",
            resolved_system_prompt
                .as_ref()
                .map(|sp| format!("--system-prompt '{}'", sp.replace('\'', "'\\''")))
                .unwrap_or_default(),
            task_prompt.replace('\'', "'\\''"),
        );
        TmuxController::send_keys(&tmux_target, &cmd).await?;
        session.status = SessionStatus::Active;
        info!("started headless claude session");
    } else {
        // Start interactive claude
        let tmux_target = format!("{}:{}", state.tmux_session_name, name);
        let cmd = claude::interactive_command(
            resolved_system_prompt.as_deref(),
            &[],
            &[],
            None,
            None,
            &config.global.claude_extra_args,
        );
        TmuxController::send_keys(&tmux_target, &cmd).await?;
        session.status = SessionStatus::Active;
        info!("started interactive claude session");
    }

    state.sessions.push(session);
    state_manager.save(&state).await?;

    info!(session = %name, "session is ready");

    Ok(())
}

/// Create worktrees for all repos in a multi-repo workspace.
///
/// Layout: `{worktree_base_dir}/{parent_name}-vibe-{short_id}/{repo_name}/`
/// Returns (session_root_path, repo_name -> worktree_path map).
async fn create_multi_repo_worktrees(
    repos: &[crate::domain::workspace::RepoInfo],
    branch_name: &str,
    base_ref: Option<&str>,
    worktree_base_dir: &Path,
    parent_name: &str,
) -> Result<(PathBuf, BTreeMap<String, PathBuf>), ForgeError> {
    let short_id = &uuid::Uuid::new_v4().to_string()[..8];
    let session_dir_name = format!("{parent_name}-vibe-{short_id}");
    let session_root = worktree_base_dir.join(&session_dir_name);

    // Create the session root directory
    tokio::fs::create_dir_all(&session_root).await?;

    let mut repo_worktrees = BTreeMap::new();
    let mut successes = 0;
    let mut errors = Vec::new();

    // Create worktrees concurrently using JoinSet
    let mut join_set = tokio::task::JoinSet::new();
    for repo in repos {
        let repo_root = repo.root.clone();
        let repo_name = repo.name.clone();
        let branch = branch_name.to_string();
        let base = base_ref.map(|s| s.to_string());
        let target_path = session_root.join(&repo_name);

        join_set.spawn(async move {
            let result = git::create_worktree_at(
                &repo_root,
                &branch,
                base.as_deref(),
                &target_path,
            )
            .await;
            (repo_name, target_path, result)
        });
    }

    while let Some(result) = join_set.join_next().await {
        match result {
            Ok((repo_name, target_path, Ok(_wt))) => {
                info!(repo = %repo_name, path = %target_path.display(), "repo worktree created");
                repo_worktrees.insert(repo_name, target_path);
                successes += 1;
            }
            Ok((repo_name, _target_path, Err(e))) => {
                warn!(repo = %repo_name, error = %e, "failed to create worktree");
                errors.push(format!("{repo_name}: {e}"));
            }
            Err(e) => {
                warn!(error = %e, "worktree task panicked");
                errors.push(format!("task panic: {e}"));
            }
        }
    }

    if successes == 0 {
        // Clean up the empty session root
        let _ = tokio::fs::remove_dir_all(&session_root).await;
        return Err(ForgeError::Git(format!(
            "Failed to create worktrees in any repo: {}",
            errors.join("; ")
        )));
    }

    if !errors.is_empty() {
        warn!(
            succeeded = successes,
            failed = errors.len(),
            "some repo worktrees failed"
        );
    }

    Ok((session_root, repo_worktrees))
}
