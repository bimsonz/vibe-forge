use crate::error::VibeError;
use crate::infra::state::StateManager;
use std::path::Path;

pub async fn execute(workspace_root: &Path, json: bool) -> Result<(), VibeError> {
    let state_manager = StateManager::new(workspace_root);
    let state = state_manager.load().await?;

    if json {
        let output = serde_json::to_string_pretty(&state)
            .map_err(|e| VibeError::State(e.to_string()))?;
        println!("{output}");
        return Ok(());
    }

    println!("Vibe: {}", state.workspace.name);
    println!("  Root: {}", state.workspace.root.display());
    if state.workspace.is_multi_repo() {
        let repo_names: Vec<&str> = state.workspace.repos.iter().map(|r| r.name.as_str()).collect();
        println!("  Repos ({}): {}", repo_names.len(), repo_names.join(", "));
    }
    println!(
        "  tmux session: {}",
        state.tmux_session_name
    );
    println!();

    let active_sessions = state.active_sessions();
    let running_agents = state.running_agents();

    println!(
        "Sessions: {} total, {} active",
        state.sessions.len(),
        active_sessions.len()
    );
    println!(
        "Agents: {} total, {} running",
        state.agents.len(),
        running_agents.len()
    );
    println!();

    if state.sessions.is_empty() {
        println!("No sessions. Run `vibe new <name>` to create one.");
        return Ok(());
    }

    for session in &state.sessions {
        let status_icon = match &session.status {
            crate::domain::session::SessionStatus::Active => "●",
            crate::domain::session::SessionStatus::Creating => "◐",
            crate::domain::session::SessionStatus::Paused => "○",
            crate::domain::session::SessionStatus::Completed => "✓",
            crate::domain::session::SessionStatus::Failed(_) => "✗",
            crate::domain::session::SessionStatus::Archived => "▪",
        };

        println!(
            "  {} {} [{}]  branch: {}",
            status_icon, session.name, session.status, session.branch,
        );

        let agents = state.agents_for_session(session.id);
        for agent in agents {
            let agent_icon = match &agent.status {
                crate::domain::agent::AgentStatus::Queued => "○",
                crate::domain::agent::AgentStatus::Running => "⟳",
                crate::domain::agent::AgentStatus::Completed => "✓",
                crate::domain::agent::AgentStatus::Failed(_) => "✗",
                crate::domain::agent::AgentStatus::Ingested => "✓",
            };
            println!(
                "    {} {} ({}) [{}]",
                agent_icon, agent.name, agent.mode, agent.status,
            );
        }
    }

    Ok(())
}
