use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use uuid::Uuid;

/// A Session represents a single Claude Code working context:
/// one git worktree + one tmux window + one claude process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub name: String,
    pub branch: String,
    pub worktree_path: PathBuf,
    pub tmux_window: String,
    pub status: SessionStatus,
    pub claude_session_id: Option<String>,
    pub template: Option<String>,
    pub system_prompt_override: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub agents: Vec<Uuid>,
    pub metadata: SessionMetadata,
    /// True for the permanent workspace-root session. Cannot be killed.
    #[serde(default)]
    pub is_main: bool,
    /// Multi-repo: worktree path per repo name. Empty for single-repo sessions.
    #[serde(default)]
    pub repo_worktrees: BTreeMap<String, PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionStatus {
    Creating,
    Active,
    Paused,
    Completed,
    Failed(String),
    Archived,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Creating => write!(f, "Creating"),
            Self::Active => write!(f, "Active"),
            Self::Paused => write!(f, "Paused"),
            Self::Completed => write!(f, "Completed"),
            Self::Failed(msg) => write!(f, "Failed: {msg}"),
            Self::Archived => write!(f, "Archived"),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub description: Option<String>,
    pub pr_number: Option<u64>,
    pub parent_session: Option<Uuid>,
    pub turns: Option<u32>,
}

impl Session {
    pub fn new(name: String, branch: String, worktree_path: PathBuf, tmux_window: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name,
            branch,
            worktree_path,
            tmux_window,
            status: SessionStatus::Creating,
            claude_session_id: None,
            template: None,
            system_prompt_override: None,
            created_at: now,
            updated_at: now,
            agents: vec![],
            metadata: SessionMetadata::default(),
            is_main: false,
            repo_worktrees: BTreeMap::new(),
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self.status, SessionStatus::Active | SessionStatus::Creating)
    }
}
