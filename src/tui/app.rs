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
    /// Next tile index to refresh incrementally (round-robin)
    pub overview_next_refresh: usize,
    /// Next agent index to reconcile incrementally (round-robin)
    pub reconcile_next_agent: usize,
    /// Deferred actions queued by key handlers to avoid blocking the event loop
    pub deferred_actions: VecDeque<DeferredAction>,
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

/// Actions queued by key handlers for processing outside the event drain loop.
/// This prevents heavy async tmux operations from blocking keyboard input.
pub enum DeferredAction {
    OpenSession,
    EnterOverview,
    KillSession { name: String },
    KillAgent {
        agent_id: uuid::Uuid,
        agent_name: String,
        agent_mode: crate::domain::agent::AgentMode,
        session_name: Option<String>,
    },
    OpenShell,
    SpawnFromTemplate {
        description: String,
        session_name: Option<String>,
        template_name: Option<String>,
        system_prompt: Option<String>,
        entry_name: String,
    },
    SpawnCustom {
        prompt: String,
        session_name: Option<String>,
    },
    CreateSession {
        name: String,
    },
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
            overview_next_refresh: 0,
            reconcile_next_agent: 0,
            deferred_actions: VecDeque::new(),
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
        self.clamp_selection_indices();
    }

    /// Ensure selection indices are within valid bounds after state changes.
    /// Prevents out-of-bounds access when sessions/agents are added or removed
    /// externally (e.g., by another vibe instance or CLI command).
    pub fn clamp_selection_indices(&mut self) {
        let session_count = self.visible_session_count();
        if session_count == 0 {
            self.selected_session = 0;
            self.selected_agent = 0;
        } else {
            if self.selected_session >= session_count {
                self.selected_session = session_count - 1;
            }
            let agent_count = self.selected_session_agents().len();
            if agent_count == 0 {
                self.selected_agent = 0;
            } else if self.selected_agent >= agent_count {
                self.selected_agent = agent_count - 1;
            }
        }
    }

    /// Full reconciliation of all agents — used at startup before the event loop.
    pub async fn reconcile_tmux_state_full(&mut self) {
        use crate::domain::agent::AgentStatus;
        use crate::infra::tmux::TmuxController;

        let mut needs_save = false;

        for agent in &mut self.state.agents {
            if agent.tmux_pane.is_none() || agent.is_done() {
                continue;
            }
            let pane_id = agent.tmux_pane.as_ref().unwrap();
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
            self.clamp_selection_indices();
        }
    }

    /// Incrementally reconcile ONE agent with tmux reality per call (round-robin).
    /// Distributes pane_exists() checks across ticks instead of blocking the
    /// event loop to check all agents at once.
    pub async fn reconcile_tmux_state(&mut self) {
        use crate::domain::agent::AgentStatus;
        use crate::infra::tmux::TmuxController;

        let agent_count = self.state.agents.len();
        if agent_count == 0 {
            self.reconcile_next_agent = 0;
            return;
        }

        // Scan forward from the round-robin index to find the next agent that
        // needs checking (has a pane and isn't done). Wrap around at most once.
        let mut checked = 0;
        while checked < agent_count {
            let idx = self.reconcile_next_agent % agent_count;
            self.reconcile_next_agent = idx + 1;
            checked += 1;

            let agent = &self.state.agents[idx];
            if agent.tmux_pane.is_none() || agent.is_done() {
                continue;
            }

            let pane_id = agent.tmux_pane.as_ref().unwrap().clone();

            if !TmuxController::pane_exists(&pane_id).await {
                tracing::info!(
                    agent = %self.state.agents[idx].name,
                    pane = %pane_id,
                    "agent pane no longer exists, marking as failed"
                );
                self.state.agents[idx].status = AgentStatus::Failed("tmux pane lost".into());
                self.state.agents[idx].tmux_pane = None;

                if let Err(e) = self.state_manager.save(&self.state).await {
                    tracing::error!(error = %e, "failed to save state after reconciliation");
                }
                self.clamp_selection_indices();
            }
            // Only check one agent per call
            return;
        }
    }
}
