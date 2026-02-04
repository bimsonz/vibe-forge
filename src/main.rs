mod cli;
mod commands;
mod config;
mod domain;
mod error;
mod infra;
mod tui;

use clap::Parser;
use cli::{Cli, Commands, ListSubcommand};
use error::ForgeError;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Resolve workspace root
    let workspace_root = cli.workspace.or_else(|| {
        let cwd = std::env::current_dir().ok()?;
        infra::git::find_repo_root(&cwd).ok()
    });

    // Preflight checks
    preflight_checks()?;

    match cli.command {
        None | Some(Commands::Dashboard) => {
            let root = workspace_root.ok_or(ForgeError::NotGitRepo)?;
            tui::run(root).await?;
        }

        Some(Commands::Init) => {
            let root = workspace_root.ok_or(ForgeError::NotGitRepo)?;
            commands::init::execute(&root).await?;
        }

        Some(Commands::New {
            name,
            branch,
            base,
            template,
            system_prompt,
            headless,
            prompt,
        }) => {
            let root = workspace_root.ok_or(ForgeError::NotGitRepo)?;
            let cfg = config::load_config(Some(&root))?;
            commands::new::execute(
                &root,
                name,
                branch,
                base,
                template,
                system_prompt,
                headless,
                prompt,
                &cfg,
            )
            .await?;
        }

        Some(Commands::Spawn {
            prompt,
            session,
            template,
            interactive,
        }) => {
            let root = workspace_root.ok_or(ForgeError::NotGitRepo)?;
            let cfg = config::load_config(Some(&root))?;
            commands::spawn::execute(&root, prompt, session, template, interactive, &cfg).await?;
        }

        Some(Commands::Status { json }) => {
            let root = workspace_root.ok_or(ForgeError::NotGitRepo)?;
            commands::status::execute(&root, json).await?;
        }

        Some(Commands::List { what }) => {
            let root = workspace_root.ok_or(ForgeError::NotGitRepo)?;
            match what {
                ListSubcommand::Sessions => {
                    commands::status::execute(&root, false).await?;
                }
                ListSubcommand::Agents { session } => {
                    // TODO: filtered agent list
                    let _ = session;
                    commands::status::execute(&root, false).await?;
                }
                ListSubcommand::Templates => {
                    let cfg = config::load_config(Some(&root))?;
                    let dirs = cfg.template_dirs(&root);
                    let templates = domain::template::AgentTemplate::load_all(&dirs);
                    println!("Available templates:");
                    for t in &templates {
                        println!("  {} ({}) - {}", t.name, t.mode, t.description);
                    }
                    if templates.is_empty() {
                        println!("  (none)");
                    }
                }
            }
        }

        Some(Commands::Kill {
            target,
            force,
            delete_branch,
        }) => {
            let root = workspace_root.ok_or(ForgeError::NotGitRepo)?;
            let cfg = config::load_config(Some(&root))?;
            commands::kill::execute(&root, target, force, delete_branch, &cfg).await?;
        }

        Some(Commands::Attach { session }) => {
            let root = workspace_root.ok_or(ForgeError::NotGitRepo)?;
            commands::attach::execute(&root, session).await?;
        }

        Some(Commands::Review { pr, interactive }) => {
            let _ = (pr, interactive);
            println!("PR review is not yet implemented (Phase 5).");
        }

        Some(Commands::Cleanup { all, dry_run }) => {
            let root = workspace_root.ok_or(ForgeError::NotGitRepo)?;
            commands::cleanup::execute(&root, all, dry_run).await?;
        }
    }

    Ok(())
}

fn preflight_checks() -> Result<(), ForgeError> {
    if !infra::tmux::TmuxController::is_available() {
        return Err(ForgeError::TmuxNotInstalled);
    }
    if !infra::claude::is_available() {
        return Err(ForgeError::ClaudeNotInstalled);
    }
    Ok(())
}
