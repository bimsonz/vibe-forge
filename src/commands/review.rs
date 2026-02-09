use crate::config::MergedConfig;
use crate::domain::agent::{Agent, AgentMode, AgentStatus};
use crate::domain::template::AgentTemplate;
use crate::error::VibeError;
use crate::infra::{claude, gh, state::StateManager, tmux::TmuxController};
use std::path::Path;

pub async fn execute(
    workspace_root: &Path,
    pr: String,
    interactive: bool,
    config: &MergedConfig,
) -> Result<(), VibeError> {
    if !gh::is_available() {
        return Err(VibeError::User(
            "gh CLI not found. Install from: https://cli.github.com".into(),
        ));
    }

    let pr_number = gh::parse_pr_identifier(&pr)?;
    println!("Fetching PR #{pr_number}...");

    // Fetch PR info and diff in parallel
    let (pr_info, diff, comments) = tokio::try_join!(
        gh::get_pr_info(pr_number, workspace_root),
        gh::get_pr_diff(pr_number, workspace_root),
        gh::get_pr_comments(pr_number, workspace_root),
    )?;

    println!("  PR: {}", pr_info.title);
    println!(
        "  {} → {}  (+{} -{})",
        pr_info.head_ref_name, pr_info.base_ref_name, pr_info.additions, pr_info.deletions
    );
    println!("  Files: {}", pr_info.files.len());

    // Build the file list
    let file_list = pr_info
        .files
        .iter()
        .map(|f| format!("  {} (+{} -{})", f.path, f.additions, f.deletions))
        .collect::<Vec<_>>()
        .join("\n");

    // Truncate diff if too large (keep under ~50k chars for context window)
    let diff_truncated = if diff.len() > 50000 {
        format!("{}...\n\n[Diff truncated at 50k chars. Read individual files for full context.]", &diff[..50000])
    } else {
        diff
    };

    // Build review prompt
    let prompt = format!(
        r#"Review PR #{pr_number}: {title}

## PR Description
{body}

## Changed Files
{file_list}

## Diff
```diff
{diff}
```

## Existing Comments/Reviews
{comments}

Please provide a thorough code review following your review process."#,
        pr_number = pr_number,
        title = pr_info.title,
        body = if pr_info.body.is_empty() { "(no description)" } else { &pr_info.body },
        file_list = file_list,
        diff = diff_truncated,
        comments = if comments.is_empty() { "(none)".to_string() } else { comments },
    );

    let state_manager = StateManager::new(workspace_root);
    let mut state = state_manager.load().await?;

    // Load reviewer template
    let template_dirs = config.template_dirs(workspace_root);
    let template = AgentTemplate::load("reviewer", &template_dirs)?;

    let session_name = format!("review-pr-{pr_number}");

    if interactive {
        // Create a full session for interactive review
        println!("Creating interactive review session...");

        // Ensure tmux session
        TmuxController::ensure_session(&state.tmux_session_name).await?;

        // Create tmux window (use main worktree, no need for separate one for review)
        let _window_id = TmuxController::create_window(
            &state.tmux_session_name,
            &session_name,
            workspace_root.to_str().unwrap_or("."),
        )
        .await?;

        // Build and send claude command with the review prompt
        let tmux_target = format!("{}:{}", state.tmux_session_name, session_name);
        let cmd = claude::interactive_command(
            config.claude_command(),
            Some(&template.system_prompt),
            &template.allowed_tools,
            &template.disallowed_tools,
            template.permission_mode.as_deref(),
            None,
            &config.global.claude_extra_args,
        );
        TmuxController::send_keys(&tmux_target, &cmd).await?;

        // Send the prompt as the first message after a brief delay
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        // Escape single quotes in prompt for shell
        let _escaped_prompt = prompt.replace('\'', "'\\''");
        // We'll write the prompt to a temp file and have claude read it
        let prompt_file = state_manager.agents_dir().join(format!("pr-{pr_number}-prompt.md"));
        tokio::fs::create_dir_all(prompt_file.parent().unwrap()).await?;
        tokio::fs::write(&prompt_file, &prompt).await?;

        println!("  Review session created: {session_name}");
        println!("  Run `vibe attach {session_name}` to interact with the reviewer");
        println!("  Prompt saved to: {}", prompt_file.display());
    } else {
        // Headless review
        println!("Spawning headless reviewer...");

        let agents_dir = state_manager.agents_dir();
        let mut agent = Agent::new(
            // Use first active session as parent, or create a synthetic one
            state
                .active_sessions()
                .first()
                .map(|s| s.id)
                .unwrap_or_else(uuid::Uuid::new_v4),
            session_name.clone(),
            AgentMode::Headless,
            prompt.clone(),
            workspace_root.to_path_buf(),
            agents_dir,
        );
        agent.template = Some("reviewer".into());
        agent.system_prompt = Some(template.system_prompt.clone());
        agent.status = AgentStatus::Running;

        let _agent_id = agent.id;
        let output_file = agent.output_file.clone();
        let output_file_display = output_file.display().to_string();
        let system_prompt = template.system_prompt.clone();
        let allowed_tools = template.allowed_tools.clone();
        let disallowed_tools = template.disallowed_tools.clone();
        let permission_mode = template.permission_mode.clone();
        let claude_cmd = config.claude_command().to_string();
        let extra_args = config.global.claude_extra_args.clone();
        let wt = workspace_root.to_path_buf();

        state.agents.push(agent);
        state_manager.save(&state).await?;

        // Run in background
        tokio::spawn(async move {
            let result = claude::run_headless(
                &claude_cmd,
                &prompt,
                &wt,
                Some(&system_prompt),
                &allowed_tools,
                &disallowed_tools,
                permission_mode.as_deref(),
                &extra_args,
            )
            .await;

            match result {
                Ok(output) => {
                    if let Some(parent) = output_file.parent() {
                        let _ = tokio::fs::create_dir_all(parent).await;
                    }
                    let json = serde_json::to_string_pretty(&output).unwrap_or_default();
                    let _ = tokio::fs::write(&output_file, &json).await;
                    println!("\nReview complete for PR #{pr_number}.");
                }
                Err(e) => {
                    eprintln!("\nReview failed for PR #{pr_number}: {e}");
                }
            }
        });

        println!("  Review agent running in background");
        println!("  Output: {output_file_display}");
        println!("  The review will auto-copy to clipboard when complete (if TUI is running)");
    }

    Ok(())
}
