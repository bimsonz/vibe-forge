use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "forge", version, about = "Multi-agent Claude Code orchestrator")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Workspace root (defaults to git repo root from cwd)
    #[arg(long, global = true)]
    pub workspace: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Launch the TUI dashboard (default when no subcommand given)
    Dashboard,

    /// Initialize forge in current repository
    Init,

    /// Create a new session (worktree + tmux window + claude)
    New {
        /// Session name (used for branch + tmux window)
        name: String,

        /// Branch name (defaults to feat/{name})
        #[arg(short, long)]
        branch: Option<String>,

        /// Base ref to create branch from (defaults to default branch)
        #[arg(long)]
        base: Option<String>,

        /// Agent template to use
        #[arg(short, long)]
        template: Option<String>,

        /// System prompt override
        #[arg(long)]
        system_prompt: Option<String>,

        /// Start in headless mode
        #[arg(long)]
        headless: bool,

        /// Initial prompt for headless mode
        #[arg(short, long)]
        prompt: Option<String>,
    },

    /// Spawn a sub-agent within an existing session
    Spawn {
        /// Task prompt for the agent
        prompt: String,

        /// Parent session name
        #[arg(short, long)]
        session: Option<String>,

        /// Agent template to use
        #[arg(short, long)]
        template: Option<String>,

        /// Run interactively instead of headless
        #[arg(long)]
        interactive: bool,
    },

    /// Show status of all sessions and agents
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// List sessions, agents, or templates
    List {
        #[command(subcommand)]
        what: ListSubcommand,
    },

    /// Kill a session or agent
    Kill {
        /// Session or agent name/ID
        target: String,

        /// Force kill without confirmation
        #[arg(short, long)]
        force: bool,

        /// Also delete the git branch
        #[arg(long)]
        delete_branch: bool,
    },

    /// Attach to a session's tmux pane
    Attach {
        /// Session name (defaults to most recent)
        session: Option<String>,
    },

    /// Spawn a PR review agent
    Review {
        /// PR number or URL
        pr: String,

        /// Run interactively
        #[arg(long)]
        interactive: bool,
    },

    /// Manage shared plan files
    Plan {
        #[command(subcommand)]
        action: PlanSubcommand,
    },

    /// Check workspace health and reconcile state
    Doctor,

    /// Clean up stale worktrees and archived sessions
    Cleanup {
        /// Remove all archived sessions
        #[arg(long)]
        all: bool,

        /// Dry run
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ListSubcommand {
    /// List all sessions
    Sessions,
    /// List agents for a session
    Agents {
        /// Session name (defaults to all)
        session: Option<String>,
    },
    /// List available templates
    Templates,
    /// List plans
    Plans,
}

#[derive(Debug, Subcommand)]
pub enum PlanSubcommand {
    /// Create a new plan
    New {
        /// Plan title
        title: String,

        /// Associated session name
        #[arg(short, long)]
        session: Option<String>,
    },
    /// List all plans
    List,
    /// View a plan by title or ID
    View {
        /// Plan title substring or UUID prefix
        query: String,
    },
    /// Copy a plan's content to clipboard
    Copy {
        /// Plan title substring or UUID prefix
        query: String,
    },
}
