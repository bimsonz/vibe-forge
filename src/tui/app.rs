use crate::config::MergedConfig;
use crate::domain::workspace::WorkspaceState;
use crate::infra::state::StateManager;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

pub struct App {
    pub workspace_root: PathBuf,
    pub state: WorkspaceState,
    pub config: MergedConfig,
    pub state_manager: StateManager,
    pub selected_session: usize,
    pub selected_agent: usize,
    pub focus: Focus,
    pub input_mode: InputMode,
    pub input_buffer: String,
    pub input_label: String,
    pub notifications: VecDeque<Notification>,
    pub output_scroll: u16,
    pub should_quit: bool,
    pub last_refresh: Instant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Focus {
    SessionList,
    AgentList,
    OutputViewer,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    NewSession,
    SpawnAgent,
    ConfirmKill,
}

pub struct Notification {
    pub message: String,
    pub level: NotifyLevel,
    pub created_at: Instant,
}

#[derive(Debug, Clone)]
pub enum NotifyLevel {
    Info,
    Success,
    Error,
}

impl App {
    pub fn new(
        workspace_root: PathBuf,
        state: WorkspaceState,
        config: MergedConfig,
        state_manager: StateManager,
    ) -> Self {
        Self {
            workspace_root,
            state,
            config,
            state_manager,
            selected_session: 0,
            selected_agent: 0,
            focus: Focus::SessionList,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            input_label: String::new(),
            notifications: VecDeque::with_capacity(10),
            output_scroll: 0,
            should_quit: false,
            last_refresh: Instant::now(),
        }
    }

    pub fn push_notification(&mut self, message: String, level: NotifyLevel) {
        self.notifications.push_back(Notification {
            message,
            level,
            created_at: Instant::now(),
        });
        // Keep only last 5
        while self.notifications.len() > 5 {
            self.notifications.pop_front();
        }
    }

    /// Get the currently selected session
    pub fn selected_session(&self) -> Option<&crate::domain::session::Session> {
        let visible: Vec<_> = self
            .state
            .sessions
            .iter()
            .filter(|s| !matches!(s.status, crate::domain::session::SessionStatus::Archived))
            .collect();
        visible.get(self.selected_session).copied()
    }

    /// Get agents for the currently selected session
    pub fn selected_session_agents(&self) -> Vec<&crate::domain::agent::Agent> {
        if let Some(session) = self.selected_session() {
            self.state.agents_for_session(session.id)
        } else {
            vec![]
        }
    }

    /// Get the currently selected agent
    pub fn selected_agent(&self) -> Option<&crate::domain::agent::Agent> {
        let agents = self.selected_session_agents();
        agents.get(self.selected_agent).copied()
    }

    /// Visible (non-archived) session count
    pub fn visible_session_count(&self) -> usize {
        self.state
            .sessions
            .iter()
            .filter(|s| !matches!(s.status, crate::domain::session::SessionStatus::Archived))
            .count()
    }

    pub async fn refresh_state(&mut self) {
        if let Ok(state) = self.state_manager.load().await {
            self.state = state;
        }
        self.last_refresh = Instant::now();
    }
}
