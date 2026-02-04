use crate::config::MergedConfig;
use crate::domain::session::{Session, SessionStatus};
use crate::domain::template::AgentTemplate;
use crate::error::ForgeError;
use crate::infra::{claude, git, state::StateManager, tmux::TmuxController};
use std::path::Path;

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
    let worktree_base_dir = config.worktree_base_dir(workspace_root);

    println!("Creating session '{name}'...");

    // Create git worktree
    let worktree = git::create_worktree(
        workspace_root,
        &branch_name,
        base_ref,
        &worktree_base_dir,
    )
    .await?;
    println!("  Worktree: {}", worktree.path.display());

    // Ensure tmux session exists
    TmuxController::ensure_session(&state.tmux_session_name).await?;

    // Create tmux window
    let window_id = TmuxController::create_window(
        &state.tmux_session_name,
        &name,
        worktree.path.to_str().unwrap_or("."),
    )
    .await?;
    println!("  tmux window: {name}");

    // Create session record
    let mut session = Session::new(
        name.clone(),
        branch_name,
        worktree.path.clone(),
        window_id.clone(),
    );

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
        println!("  Started headless claude session");
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
        println!("  Started interactive claude session");
    }

    state.sessions.push(session);
    state_manager.save(&state).await?;

    println!("\nSession '{name}' is ready.");
    println!("  Run `forge attach {name}` to switch to it");

    Ok(())
}
