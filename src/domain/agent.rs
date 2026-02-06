use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// An Agent is a sub-task spawned from a Session. Can be headless or interactive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: Uuid,
    pub parent_session: Uuid,
    pub name: String,
    pub mode: AgentMode,
    pub status: AgentStatus,
    pub template: Option<String>,
    pub prompt: String,
    pub system_prompt: Option<String>,
    pub worktree_path: PathBuf,
    pub tmux_pane: Option<String>,
    pub pid: Option<u32>,
    pub claude_session_id: Option<String>,
    pub output_file: PathBuf,
    pub result: Option<AgentResult>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AgentMode {
    Headless,
    Interactive,
    Shell,
}

impl std::fmt::Display for AgentMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Headless => write!(f, "headless"),
            Self::Interactive => write!(f, "interactive"),
            Self::Shell => write!(f, "shell"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AgentStatus {
    Queued,
    Running,
    Completed,
    Failed(String),
    Ingested,
}

impl std::fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Queued => write!(f, "Queued"),
            Self::Running => write!(f, "Running"),
            Self::Completed => write!(f, "Completed"),
            Self::Failed(msg) => write!(f, "Failed: {msg}"),
            Self::Ingested => write!(f, "Ingested"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    pub success: bool,
    pub summary: String,
    pub duration_ms: u64,
    pub session_id: String,
    pub raw_result: Option<String>,
}

impl Agent {
    pub fn new(
        parent_session: Uuid,
        name: String,
        mode: AgentMode,
        prompt: String,
        worktree_path: PathBuf,
        output_dir: PathBuf,
    ) -> Self {
        let id = Uuid::new_v4();
        let output_file = output_dir.join(id.to_string()).join("output.json");
        Self {
            id,
            parent_session,
            name,
            mode,
            status: AgentStatus::Queued,
            template: None,
            prompt,
            system_prompt: None,
            worktree_path,
            tmux_pane: None,
            pid: None,
            claude_session_id: None,
            output_file,
            result: None,
            created_at: Utc::now(),
            completed_at: None,
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self.status, AgentStatus::Running)
    }

    pub fn is_done(&self) -> bool {
        matches!(
            self.status,
            AgentStatus::Completed | AgentStatus::Failed(_) | AgentStatus::Ingested
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_agent() -> Agent {
        Agent::new(
            Uuid::new_v4(),
            "test-agent".into(),
            AgentMode::Headless,
            "do something".into(),
            PathBuf::from("/tmp/test"),
            PathBuf::from("/tmp/agents"),
        )
    }

    #[test]
    fn test_agent_new_defaults() {
        let agent = make_agent();
        assert_eq!(agent.name, "test-agent");
        assert_eq!(agent.mode, AgentMode::Headless);
        assert_eq!(agent.status, AgentStatus::Queued);
        assert!(agent.template.is_none());
        assert!(agent.result.is_none());
        assert!(agent.completed_at.is_none());
        assert!(agent.output_file.to_string_lossy().contains("output.json"));
    }

    #[test]
    fn test_is_running() {
        let mut agent = make_agent();
        assert!(!agent.is_running());
        agent.status = AgentStatus::Running;
        assert!(agent.is_running());
        agent.status = AgentStatus::Completed;
        assert!(!agent.is_running());
    }

    #[test]
    fn test_is_done() {
        let mut agent = make_agent();
        assert!(!agent.is_done());
        agent.status = AgentStatus::Running;
        assert!(!agent.is_done());
        agent.status = AgentStatus::Completed;
        assert!(agent.is_done());
        agent.status = AgentStatus::Failed("err".into());
        assert!(agent.is_done());
        agent.status = AgentStatus::Ingested;
        assert!(agent.is_done());
    }

    #[test]
    fn test_agent_serialization_roundtrip() {
        let agent = make_agent();
        let json = serde_json::to_string(&agent).unwrap();
        let deserialized: Agent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, agent.id);
        assert_eq!(deserialized.name, agent.name);
        assert_eq!(deserialized.status, agent.status);
    }
}
