use crate::config::MergedConfig;
use crate::domain::workspace::WorkspaceState;
use crate::infra::state::StateManager;
use ratatui::style::Color;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;
use uuid::Uuid;

pub struct App {
    pub workspace_root: PathBuf,
    pub state: WorkspaceState,
    pub config: MergedConfig,
    pub state_manager: StateManager,
    pub selected_session: usize,
    pub selected_agent: usize,
    pub focus: Focus,
    pub view_mode: ViewMode,
    pub input_mode: InputMode,
    pub input_buffer: String,
    pub input_label: String,
    pub agent_entries: Vec<AgentEntry>,
    pub selected_template: usize,
    pub notifications: VecDeque<Notification>,
    pub output_scroll: u16,
    pub last_refresh: Instant,
    /// Captured pane content for overview tiles
    pub overview_captures: Vec<OverviewTile>,
    /// Currently selected tile in overview
    pub overview_selected: usize,
    /// Last time overview captures were refreshed
    pub overview_last_capture: Instant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Focus {
    SessionList,
    AgentList,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ViewMode {
    Dashboard,
    AgentOutput,
    SessionOverview,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    NewSession,
    SpawnAgent,
    SelectTemplate,
    ConfirmKillSession,
    ConfirmKillAgent,
}

pub struct AgentEntry {
    pub name: String,
    pub description: String,
    pub source: AgentSource,
}

pub enum AgentSource {
    /// Open a bare shell pane in the session's tmux window
    Shell,
    /// Claude Code .claude/agents/*.md — contains the full system prompt
    ClaudeCode(String),
    /// Forge template — name matches a forge template to pass to spawn
    ForgeTemplate,
}

pub struct Notification {
    pub message: String,
    pub level: NotifyLevel,
    pub created_at: Instant,
}

#[derive(Clone)]
pub struct OverviewTile {
    pub session_name: String,
    pub pane_id: String,
    pub content: String,
    pub color: Color,
}

#[derive(Debug, Clone)]
pub enum NotifyLevel {
    Info,
    Success,
    Error,
}

const SESSION_PALETTE: [Color; 10] = [
    Color::Cyan,
    Color::Magenta,
    Color::Yellow,
    Color::Green,
    Color::Blue,
    Color::LightCyan,
    Color::LightMagenta,
    Color::LightGreen,
    Color::LightRed,
    Color::LightBlue,
];

/// Deterministic color for a session based on its UUID.
pub fn session_color(id: Uuid) -> Color {
    let index = id.as_bytes()[0] as usize % SESSION_PALETTE.len();
    SESSION_PALETTE[index]
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
            view_mode: ViewMode::Dashboard,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            input_label: String::new(),
            agent_entries: vec![],
            selected_template: 0,
            notifications: VecDeque::with_capacity(10),
            output_scroll: 0,
            last_refresh: Instant::now(),
            overview_captures: vec![],
            overview_selected: 0,
            overview_last_capture: Instant::now(),
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

    /// Visible sessions in display order (main pinned first, then rest).
    pub fn visible_sessions(&self) -> Vec<&crate::domain::session::Session> {
        let mut visible: Vec<_> = self
            .state
            .sessions
            .iter()
            .filter(|s| !matches!(s.status, crate::domain::session::SessionStatus::Archived))
            .collect();
        visible.sort_by_key(|s| if s.is_main { 0 } else { 1 });
        visible
    }

    /// Get the currently selected session
    pub fn selected_session(&self) -> Option<&crate::domain::session::Session> {
        self.visible_sessions().get(self.selected_session).copied()
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

    /// Color for the currently selected session, falls back to Gray.
    pub fn current_session_color(&self) -> Color {
        self.selected_session()
            .map(|s| session_color(s.id))
            .unwrap_or(Color::Gray)
    }

    pub async fn refresh_state(&mut self) {
        if let Ok(state) = self.state_manager.load().await {
            self.state = state;
        }
        self.last_refresh = Instant::now();
    }

    /// Reconcile state with tmux reality.
    /// Validates that pane IDs in state actually exist, updates status if not.
    pub async fn reconcile_tmux_state(&mut self) {
        use crate::domain::agent::AgentStatus;
        use crate::infra::tmux::TmuxController;

        let mut needs_save = false;

        for agent in &mut self.state.agents {
            // Skip agents without panes or already done
            if agent.tmux_pane.is_none() || agent.is_done() {
                continue;
            }

            let pane_id = agent.tmux_pane.as_ref().unwrap();

            // Check if pane still exists
            if !TmuxController::pane_exists(pane_id).await {
                tracing::info!(
                    agent = %agent.name,
                    pane = %pane_id,
                    "agent pane no longer exists, marking as failed"
                );
                agent.status = AgentStatus::Failed("tmux pane lost".into());
                agent.tmux_pane = None;
                needs_save = true;
            }
        }

        if needs_save {
            if let Err(e) = self.state_manager.save(&self.state).await {
                tracing::error!(error = %e, "failed to save state after reconciliation");
            }
        }
    }
}
