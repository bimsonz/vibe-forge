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

/// Persisted state for a workspace: .forge/workspace.json
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
