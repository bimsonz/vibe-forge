mod cli;
mod commands;
mod config;
mod domain;
mod error;
mod infra;
mod tui;

use clap::Parser;
use cli::{Cli, Commands, ListSubcommand, PlanSubcommand};
use error::ForgeError;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Resolve workspace root.
    // 1. Explicit --workspace flag
    // 2. cwd is inside a git repo → use the repo root
    // 3. cwd has .vibe/ → multi-repo workspace already initialized here
    // 4. cwd contains git sub-directories → potential multi-repo workspace
    let workspace_root = cli.workspace.or_else(|| {
        let cwd = std::env::current_dir().ok()?;
        // Try git repo first (single-repo mode)
        if let Ok(root) = infra::git::find_repo_root(&cwd) {
            return Some(root);
        }
        // Already initialized multi-repo workspace
        if cwd.join(".vibe").exists() {
            return Some(cwd);
        }
        // Not yet initialized — check if there are git repos here (for `vibe init`)
        if let Ok(repos) = infra::git::discover_repos(&cwd) {
            if !repos.is_empty() {
                return Some(cwd);
            }
        }
        None
    });

    // Initialize tracing (log to .vibe/vibe.log if workspace exists)
    let _guard = init_tracing(workspace_root.as_deref());

    // Preflight checks
    preflight_checks()?;

    info!(
        command = ?cli.command,
        workspace = ?workspace_root,
        "vibe started"
    );

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
            commands::spawn::execute(&root, prompt, session, template, None, interactive, &cfg).await?;
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
                ListSubcommand::Plans => {
                    commands::plan::list(&root).await?;
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
            let root = workspace_root.ok_or(ForgeError::NotGitRepo)?;
            let cfg = config::load_config(Some(&root))?;
            commands::review::execute(&root, pr, interactive, &cfg).await?;
        }

        Some(Commands::Plan { action }) => {
            let root = workspace_root.ok_or(ForgeError::NotGitRepo)?;
            match action {
                PlanSubcommand::New { title, session } => {
                    commands::plan::create(&root, title, session).await?;
                }
                PlanSubcommand::List => {
                    commands::plan::list(&root).await?;
                }
                PlanSubcommand::View { query } => {
                    commands::plan::view(&root, query).await?;
                }
                PlanSubcommand::Copy { query } => {
                    commands::plan::copy(&root, query).await?;
                }
            }
        }

        Some(Commands::Doctor) => {
            let root = workspace_root.ok_or(ForgeError::NotGitRepo)?;
            commands::doctor::execute(&root).await?;
        }

        Some(Commands::Cleanup { all, dry_run }) => {
            let root = workspace_root.ok_or(ForgeError::NotGitRepo)?;
            commands::cleanup::execute(&root, all, dry_run).await?;
        }

        Some(Commands::RefreshRepos) => {
            let root = workspace_root.ok_or(ForgeError::NotGitRepo)?;
            commands::refresh_repos::execute(&root).await?;
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

/// Initialize tracing with a file appender. Returns a guard that must be held
/// for the lifetime of the program (dropping it flushes the writer).
fn init_tracing(
    workspace_root: Option<&std::path::Path>,
) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::{fmt, EnvFilter};

    let log_dir = workspace_root.map(|r| r.join(".vibe"));
    let log_dir = match log_dir {
        Some(d) if d.exists() => d,
        _ => return None,
    };

    let file_appender = tracing_appender::rolling::never(&log_dir, "vibe.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(false)
        .init();

    Some(guard)
}
