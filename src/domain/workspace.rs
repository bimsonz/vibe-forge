use crate::domain::agent::Agent;
use crate::domain::session::Session;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Workspace represents the root git repository that forge manages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub root: PathBuf,
    pub name: String,
    pub default_branch: String,
    pub remote_url: Option<String>,
    pub worktree_prefix: String,
    pub worktree_base_dir: PathBuf,
}

/// Persisted state for a workspace: .vibe/workspace.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceState {
    pub workspace: Workspace,
    pub sessions: Vec<Session>,
    pub agents: Vec<Agent>,
    pub tmux_session_name: String,
}

impl WorkspaceState {
    pub fn new(workspace: Workspace, tmux_session_name: String) -> Self {
        Self {
            workspace,
            sessions: vec![],
            agents: vec![],
            tmux_session_name,
        }
    }

    pub fn find_session_by_name(&self, name: &str) -> Option<&Session> {
        self.sessions.iter().find(|s| s.name == name)
    }

    pub fn find_session_by_name_mut(&mut self, name: &str) -> Option<&mut Session> {
        self.sessions.iter_mut().find(|s| s.name == name)
    }

    pub fn find_session_by_id(&self, id: Uuid) -> Option<&Session> {
        self.sessions.iter().find(|s| s.id == id)
    }

    pub fn find_session_by_id_mut(&mut self, id: Uuid) -> Option<&mut Session> {
        self.sessions.iter_mut().find(|s| s.id == id)
    }

    pub fn find_agent_by_id(&self, id: Uuid) -> Option<&Agent> {
        self.agents.iter().find(|a| a.id == id)
    }

    pub fn find_agent_by_id_mut(&mut self, id: Uuid) -> Option<&mut Agent> {
        self.agents.iter_mut().find(|a| a.id == id)
    }

    pub fn agents_for_session(&self, session_id: Uuid) -> Vec<&Agent> {
        self.agents
            .iter()
            .filter(|a| a.parent_session == session_id)
            .collect()
    }

    pub fn active_sessions(&self) -> Vec<&Session> {
        self.sessions.iter().filter(|s| s.is_active()).collect()
    }

    pub fn running_agents(&self) -> Vec<&Agent> {
        self.agents.iter().filter(|a| a.is_running()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::agent::{Agent, AgentMode, AgentStatus};
    use crate::domain::session::{Session, SessionStatus};

    fn make_workspace() -> Workspace {
        Workspace {
            root: PathBuf::from("/tmp/test-repo"),
            name: "test-repo".into(),
            default_branch: "main".into(),
            remote_url: None,
            worktree_prefix: "test-repo-forge".into(),
            worktree_base_dir: PathBuf::from("/tmp"),
        }
    }

    fn make_session(name: &str) -> Session {
        Session::new(
            name.into(),
            format!("feat/{name}"),
            PathBuf::from(format!("/tmp/{name}")),
            name.into(),
        )
    }

    fn make_agent(session_id: Uuid, name: &str) -> Agent {
        Agent::new(
            session_id,
            name.into(),
            AgentMode::Headless,
            "test prompt".into(),
            PathBuf::from("/tmp/test"),
            PathBuf::from("/tmp/agents"),
        )
    }

    #[test]
    fn test_workspace_state_new() {
        let ws = make_workspace();
        let state = WorkspaceState::new(ws, "forge-test".into());
        assert!(state.sessions.is_empty());
        assert!(state.agents.is_empty());
        assert_eq!(state.tmux_session_name, "forge-test");
    }

    #[test]
    fn test_find_session_by_name() {
        let ws = make_workspace();
        let mut state = WorkspaceState::new(ws, "forge-test".into());
        let session = make_session("my-feature");
        state.sessions.push(session);

        assert!(state.find_session_by_name("my-feature").is_some());
        assert!(state.find_session_by_name("nonexistent").is_none());
    }

    #[test]
    fn test_active_sessions() {
        let ws = make_workspace();
        let mut state = WorkspaceState::new(ws, "forge-test".into());

        let mut s1 = make_session("active");
        s1.status = SessionStatus::Active;
        let mut s2 = make_session("archived");
        s2.status = SessionStatus::Archived;
        let s3 = make_session("creating"); // default is Creating

        state.sessions.push(s1);
        state.sessions.push(s2);
        state.sessions.push(s3);

        let active = state.active_sessions();
        assert_eq!(active.len(), 2); // Active + Creating
    }

    #[test]
    fn test_agents_for_session() {
        let ws = make_workspace();
        let mut state = WorkspaceState::new(ws, "forge-test".into());

        let session = make_session("my-feature");
        let session_id = session.id;
        state.sessions.push(session);

        let a1 = make_agent(session_id, "agent-1");
        let a2 = make_agent(session_id, "agent-2");
        let a3 = make_agent(Uuid::new_v4(), "other-agent");

        state.agents.push(a1);
        state.agents.push(a2);
        state.agents.push(a3);

        let session_agents = state.agents_for_session(session_id);
        assert_eq!(session_agents.len(), 2);
    }

    #[test]
    fn test_running_agents() {
        let ws = make_workspace();
        let mut state = WorkspaceState::new(ws, "forge-test".into());

        let session_id = Uuid::new_v4();
        let mut a1 = make_agent(session_id, "running");
        a1.status = AgentStatus::Running;
        let mut a2 = make_agent(session_id, "completed");
        a2.status = AgentStatus::Completed;

        state.agents.push(a1);
        state.agents.push(a2);

        assert_eq!(state.running_agents().len(), 1);
    }

    #[test]
    fn test_state_serialization_roundtrip() {
        let ws = make_workspace();
        let mut state = WorkspaceState::new(ws, "forge-test".into());
        state.sessions.push(make_session("test"));

        let json = serde_json::to_string(&state).unwrap();
        let deserialized: WorkspaceState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.sessions.len(), 1);
        assert_eq!(deserialized.sessions[0].name, "test");
    }
}
