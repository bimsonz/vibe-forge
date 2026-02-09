use crate::domain::agent::AgentResult;
use crate::infra::claude::ClaudeJsonOutput;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use tokio::sync::mpsc;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug)]
pub enum WatcherEvent {
    AgentCompleted { agent_id: Uuid, result: AgentResult },
    AgentOutputWritten { path: PathBuf },
}

pub struct VibeWatcher {
    _watcher: RecommendedWatcher,
}

impl VibeWatcher {
    /// Watch .vibe/agents/ for output.json files being created.
    /// Uses a bounded channel sender to prevent unbounded memory growth
    /// if events arrive faster than the TUI can drain them.
    pub fn start(
        agents_dir: PathBuf,
        tx: mpsc::Sender<WatcherEvent>,
    ) -> Result<Self, notify::Error> {
        // Ensure directory exists
        let _ = std::fs::create_dir_all(&agents_dir);
        info!(dir = %agents_dir.display(), "starting file watcher");

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    match event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) => {
                            for path in &event.paths {
                                if path.file_name().is_some_and(|n| n == "output.json") {
                                    // Extract agent UUID from parent directory name
                                    if let Some(agent_id) = path
                                        .parent()
                                        .and_then(|p| p.file_name())
                                        .and_then(|n| n.to_str())
                                        .and_then(|s| Uuid::parse_str(s).ok())
                                    {
                                        // Try to parse the output
                                        if let Ok(content) = std::fs::read_to_string(path) {
                                            if let Ok(output) =
                                                serde_json::from_str::<ClaudeJsonOutput>(&content)
                                            {
                                                info!(%agent_id, "agent output detected");
                                                let result =
                                                    crate::infra::claude::to_agent_result(&output);
                                                // try_send: drop event if buffer full rather than OOM
                                                let _ = tx.try_send(WatcherEvent::AgentCompleted {
                                                    agent_id,
                                                    result,
                                                });
                                            } else {
                                                warn!(%agent_id, "failed to parse agent output JSON");
                                            }
                                        }
                                    } else {
                                        let _ = tx.try_send(WatcherEvent::AgentOutputWritten {
                                            path: path.clone(),
                                        });
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            },
            Config::default(),
        )?;

        watcher.watch(&agents_dir, RecursiveMode::Recursive)?;

        Ok(Self { _watcher: watcher })
    }
}
