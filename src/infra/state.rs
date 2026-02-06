use crate::domain::workspace::WorkspaceState;
use crate::error::ForgeError;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info};

pub struct StateManager {
    vibe_dir: PathBuf,
    state_file: PathBuf,
}

impl StateManager {
    pub fn new(workspace_root: &Path) -> Self {
        let vibe_dir = workspace_root.join(".vibe");
        let state_file = vibe_dir.join("workspace.json");
        Self {
            vibe_dir,
            state_file,
        }
    }

    pub fn vibe_dir(&self) -> &Path {
        &self.vibe_dir
    }

    pub fn agents_dir(&self) -> PathBuf {
        self.vibe_dir.join("agents")
    }

    pub fn plans_dir(&self) -> PathBuf {
        self.vibe_dir.join("plans")
    }

    /// Initialize .vibe directory structure
    pub async fn init(&self) -> Result<(), ForgeError> {
        info!(dir = %self.vibe_dir.display(), "initializing .vibe directory");
        fs::create_dir_all(&self.vibe_dir).await?;
        fs::create_dir_all(self.vibe_dir.join("agents")).await?;
        fs::create_dir_all(self.vibe_dir.join("plans")).await?;
        fs::create_dir_all(self.vibe_dir.join("templates")).await?;
        self.ensure_gitignore().await?;
        Ok(())
    }

    /// Check if forge is initialized
    pub fn is_initialized(&self) -> bool {
        self.vibe_dir.exists() && self.state_file.exists()
    }

    /// Load state from disk
    pub async fn load(&self) -> Result<WorkspaceState, ForgeError> {
        if !self.state_file.exists() {
            return Err(ForgeError::NotInitialized);
        }
        let content = fs::read_to_string(&self.state_file).await?;
        let state: WorkspaceState =
            serde_json::from_str(&content).map_err(|e| ForgeError::State(e.to_string()))?;
        Ok(state)
    }

    /// Persist state to disk (atomic write via temp file + rename)
    pub async fn save(&self, state: &WorkspaceState) -> Result<(), ForgeError> {
        debug!(
            sessions = state.sessions.len(),
            agents = state.agents.len(),
            "saving workspace state"
        );
        let json = serde_json::to_string_pretty(state)
            .map_err(|e| ForgeError::State(e.to_string()))?;
        let tmp = self.state_file.with_extension("json.tmp");
        fs::write(&tmp, &json).await?;
        fs::rename(&tmp, &self.state_file).await?;
        Ok(())
    }

    /// Save agent output to .vibe/agents/{id}/
    pub async fn save_agent_output(
        &self,
        agent_id: &uuid::Uuid,
        output: &str,
    ) -> Result<PathBuf, ForgeError> {
        let agent_dir = self.agents_dir().join(agent_id.to_string());
        fs::create_dir_all(&agent_dir).await?;
        let output_file = agent_dir.join("output.json");
        fs::write(&output_file, output).await?;
        Ok(output_file)
    }

    async fn ensure_gitignore(&self) -> Result<(), ForgeError> {
        let gitignore = self
            .vibe_dir
            .parent()
            .expect("vibe_dir has a parent")
            .join(".gitignore");

        if gitignore.exists() {
            let content = fs::read_to_string(&gitignore).await?;
            if !content.contains(".vibe/") {
                let mut file = fs::OpenOptions::new()
                    .append(true)
                    .open(&gitignore)
                    .await?;
                file.write_all(b"\n# Vibe agent orchestrator\n.vibe/\n")
                    .await?;
            }
        } else {
            fs::write(&gitignore, "# Vibe agent orchestrator\n.vibe/\n").await?;
        }
        Ok(())
    }
}
