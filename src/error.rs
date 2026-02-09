use thiserror::Error;

#[derive(Error, Debug)]
pub enum VibeError {
    #[error("Git error: {0}")]
    Git(String),

    #[error("Git2 library error: {0}")]
    Git2(#[from] git2::Error),

    #[error("tmux error: {0}")]
    Tmux(String),

    #[error("Claude CLI error: {0}")]
    Claude(String),

    #[error("Template error: {0}")]
    Template(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("State error: {0}")]
    State(String),

    #[error("Clipboard error: {0}")]
    Clipboard(String),

    #[error("File watcher error: {0}")]
    Watcher(#[from] notify::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Agent not found: {0}")]
    AgentNotFound(String),

    #[error("Not a git repository")]
    NotGitRepo,

    #[error("No git repositories found in directory")]
    NoReposFound,

    #[error("Vibe not initialized. Run `vibe init` first.")]
    NotInitialized,

    #[error("tmux not installed. Install with: brew install tmux")]
    TmuxNotInstalled,

    #[error("claude CLI not found. Install from: https://claude.ai/code")]
    ClaudeNotInstalled,

    #[error("{0}")]
    User(String),
}
