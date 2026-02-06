use crate::config::MergedConfig;
use crate::domain::agent::{Agent, AgentMode, AgentStatus};
use crate::domain::template::AgentTemplate;
use crate::error::ForgeError;
use crate::infra::{claude, state::StateManager, tmux::TmuxController};
use std::path::Path;
use tracing::{error, info};

pub async fn execute(
    workspace_root: &Path,
    prompt: String,
    session_name: Option<String>,
    template_name: Option<String>,
    system_prompt_override: Option<String>,
    interactive: bool,
    config: &MergedConfig,
) -> Result<(), ForgeError> {
    let state_manager = StateManager::new(workspace_root);
    let mut state = state_manager.load().await?;

    // Find parent session
    let parent = if let Some(ref name) = session_name {
        state
            .find_session_by_name(name)
            .ok_or_else(|| ForgeError::SessionNotFound(name.clone()))?
    } else {
        // Default to the most recently created active session
        state
            .active_sessions()
            .into_iter()
            .max_by_key(|s| s.created_at)
            .ok_or_else(|| ForgeError::User("No active sessions found".into()))?
    };

    let parent_id = parent.id;
    let parent_name = parent.name.clone();
    let worktree_path = parent.worktree_path.clone();

    // Load template
    let template = if let Some(ref name) = template_name {
        let dirs = config.template_dirs(workspace_root);
        Some(AgentTemplate::load(name, &dirs)?)
    } else {
        None
    };

    let mode = if interactive {
        AgentMode::Interactive
    } else {
        template
            .as_ref()
            .map(|t| t.mode.clone())
            .unwrap_or(AgentMode::Headless)
    };

    // Create agent name from template or prompt
    let agent_name = template_name
        .as_deref()
        .unwrap_or("agent")
        .to_string();

    let agents_dir = state_manager.agents_dir();
    let mut agent = Agent::new(
        parent_id,
        agent_name.clone(),
        mode.clone(),
        prompt.clone(),
        worktree_path.clone(),
        agents_dir,
    );

    // Apply system prompt: explicit override > template > none
    if let Some(ref sp) = system_prompt_override {
        agent.system_prompt = Some(sp.clone());
    } else if let Some(ref tmpl) = template {
        agent.template = Some(tmpl.name.clone());
        agent.system_prompt = Some(tmpl.system_prompt.clone());
    }

    info!(
        mode = %mode,
        agent = %agent_name,
        session = %parent_name,
        "spawning agent"
    );

    match mode {
        AgentMode::Headless => {
            agent.status = AgentStatus::Running;
            let agent_id = agent.id;
            let system_prompt = agent.system_prompt.clone();
            let output_file = agent.output_file.clone();
            let allowed_tools = template
                .as_ref()
                .map(|t| t.allowed_tools.clone())
                .unwrap_or_default();
            let disallowed_tools = template
                .as_ref()
                .map(|t| t.disallowed_tools.clone())
                .unwrap_or_default();
            let permission_mode = template.as_ref().and_then(|t| t.permission_mode.clone());
            let extra_args = config.global.claude_extra_args.clone();
            let wt = worktree_path.clone();

            state.agents.push(agent);

            // Add agent ID to parent session
            if let Some(parent) = state.find_session_by_id_mut(parent_id) {
                parent.agents.push(agent_id);
            }
            state_manager.save(&state).await?;

            info!(output = %output_file.display(), "agent running in background");

            // Spawn headless in background
            tokio::spawn(async move {
                let result = claude::run_headless(
                    &prompt,
                    &wt,
                    system_prompt.as_deref(),
                    &allowed_tools,
                    &disallowed_tools,
                    permission_mode.as_deref(),
                    &extra_args,
                )
                .await;

                match result {
                    Ok(output) => {
                        // Write output to agent dir
                        if let Some(parent) = output_file.parent() {
                            let _ = tokio::fs::create_dir_all(parent).await;
                        }
                        let json = serde_json::to_string_pretty(&output).unwrap_or_default();
                        let _ = tokio::fs::write(&output_file, &json).await;
                        info!(agent = %agent_name, "agent completed");
                    }
                    Err(e) => {
                        error!(agent = %agent_name, error = %e, "agent failed");
                    }
                }
            });
        }
        AgentMode::Interactive => {
            let tmux_target = format!("{}:{}", state.tmux_session_name, parent_name);
            let pane_id = TmuxController::split_pane(
                &tmux_target,
                worktree_path.to_str().unwrap_or("."),
                true,
            )
            .await?;

            agent.tmux_pane = Some(pane_id.clone());
            agent.status = AgentStatus::Running;

            let cmd = claude::interactive_command(
                agent.system_prompt.as_deref(),
                &template
                    .as_ref()
                    .map(|t| t.allowed_tools.clone())
                    .unwrap_or_default(),
                &template
                    .as_ref()
                    .map(|t| t.disallowed_tools.clone())
                    .unwrap_or_default(),
                template.as_ref().and_then(|t| t.permission_mode.as_deref()),
                None,
                &config.global.claude_extra_args,
            );
            TmuxController::send_keys(&pane_id, &cmd).await?;

            let agent_id = agent.id;
            state.agents.push(agent);
            if let Some(parent) = state.find_session_by_id_mut(parent_id) {
                parent.agents.push(agent_id);
            }
            state_manager.save(&state).await?;

            info!(pane = %pane_id, "interactive agent started");
        }
        AgentMode::Shell => {
            // Shells are created directly by the TUI, not via spawn command.
            return Err(ForgeError::Tmux("Shell agents cannot be spawned via CLI".into()));
        }
    }

    Ok(())
}
