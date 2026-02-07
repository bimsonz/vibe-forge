use crate::domain::agent::Agent;
use crate::domain::session::Session;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Whether the workspace manages a single git repo or multiple.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum WorkspaceKind {
    #[default]
    SingleRepo,
    MultiRepo,
}

/// Metadata about a single git repository within a multi-repo workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoInfo {
    pub root: PathBuf,
    /// Directory name relative to parent (e.g. "api", "web")
    pub name: String,
    pub default_branch: String,
    pub remote_url: Option<String>,
}

/// Workspace represents the root directory that vibe manages.
/// For SingleRepo this is the git repo root. For MultiRepo this is the
/// parent directory containing multiple git repos.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub root: PathBuf,
    pub name: String,
    pub default_branch: String,
    pub remote_url: Option<String>,
    pub worktree_prefix: String,
    pub worktree_base_dir: PathBuf,
    #[serde(default)]
    pub kind: WorkspaceKind,
    #[serde(default)]
    pub repos: Vec<RepoInfo>,
}

/// Persisted state for a workspace: .vibe/workspace.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceState {
    pub workspace: Workspace,
    pub sessions: Vec<Session>,
    pub agents: Vec<Agent>,
    pub tmux_session_name: String,
}

impl Workspace {
    pub fn is_multi_repo(&self) -> bool {
        self.kind == WorkspaceKind::MultiRepo
    }
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
            kind: Default::default(),
            repos: vec![],
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

    /// Existing single-repo workspace.json (without kind/repos/repo_worktrees)
    /// must deserialize correctly with defaults.
    #[test]
    fn test_backwards_compat_deserialization() {
        // Simulate an old workspace.json without the new fields
        let old_json = r#"{
            "workspace": {
                "root": "/tmp/test-repo",
                "name": "test-repo",
                "default_branch": "main",
                "remote_url": null,
                "worktree_prefix": "-vibe-",
                "worktree_base_dir": "/tmp"
            },
            "sessions": [{
                "id": "00000000-0000-0000-0000-000000000001",
                "name": "my-feature",
                "branch": "feat/my-feature",
                "worktree_path": "/tmp/my-feature",
                "tmux_window": "@1",
                "status": "Active",
                "claude_session_id": null,
                "template": null,
                "system_prompt_override": null,
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z",
                "agents": [],
                "metadata": {
                    "description": null,
                    "pr_number": null,
                    "parent_session": null,
                    "turns": null
                }
            }],
            "agents": [],
            "tmux_session_name": "vibe-test"
        }"#;

        let state: WorkspaceState = serde_json::from_str(old_json).unwrap();
        assert_eq!(state.workspace.kind, WorkspaceKind::SingleRepo);
        assert!(state.workspace.repos.is_empty());
        assert!(!state.workspace.is_multi_repo());
        assert!(state.sessions[0].repo_worktrees.is_empty());
        assert!(!state.sessions[0].is_main); // default false
    }

    #[test]
    fn test_multi_repo_serialization_roundtrip() {
        let ws = Workspace {
            root: PathBuf::from("/tmp/my-platform"),
            name: "my-platform".into(),
            default_branch: "main".into(),
            remote_url: None,
            worktree_prefix: "-vibe-".into(),
            worktree_base_dir: PathBuf::from("/tmp"),
            kind: WorkspaceKind::MultiRepo,
            repos: vec![
                RepoInfo {
                    root: PathBuf::from("/tmp/my-platform/api"),
                    name: "api".into(),
                    default_branch: "main".into(),
                    remote_url: Some("git@github.com:org/api.git".into()),
                },
                RepoInfo {
                    root: PathBuf::from("/tmp/my-platform/web"),
                    name: "web".into(),
                    default_branch: "main".into(),
                    remote_url: None,
                },
            ],
        };

        let mut state = WorkspaceState::new(ws, "vibe-my-platform".into());
        let mut session = make_session("onboarding");
        session.repo_worktrees.insert(
            "api".into(),
            PathBuf::from("/tmp/my-platform-vibe-abc12345/api"),
        );
        session.repo_worktrees.insert(
            "web".into(),
            PathBuf::from("/tmp/my-platform-vibe-abc12345/web"),
        );
        state.sessions.push(session);

        let json = serde_json::to_string_pretty(&state).unwrap();
        let deserialized: WorkspaceState = serde_json::from_str(&json).unwrap();

        assert!(deserialized.workspace.is_multi_repo());
        assert_eq!(deserialized.workspace.repos.len(), 2);
        assert_eq!(deserialized.sessions[0].repo_worktrees.len(), 2);
        assert!(deserialized.sessions[0].repo_worktrees.contains_key("api"));
        assert!(deserialized.sessions[0].repo_worktrees.contains_key("web"));
    }
}
