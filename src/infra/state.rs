use crate::domain::workspace::WorkspaceState;
use crate::error::ForgeError;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

pub struct StateManager {
    forge_dir: PathBuf,
    state_file: PathBuf,
}

impl StateManager {
    pub fn new(workspace_root: &Path) -> Self {
        let forge_dir = workspace_root.join(".forge");
        let state_file = forge_dir.join("workspace.json");
        Self {
            forge_dir,
            state_file,
        }
    }

    pub fn forge_dir(&self) -> &Path {
        &self.forge_dir
    }

    pub fn agents_dir(&self) -> PathBuf {
        self.forge_dir.join("agents")
    }

    pub fn plans_dir(&self) -> PathBuf {
        self.forge_dir.join("plans")
    }

    /// Initialize .forge directory structure
    pub async fn init(&self) -> Result<(), ForgeError> {
        fs::create_dir_all(&self.forge_dir).await?;
        fs::create_dir_all(self.forge_dir.join("agents")).await?;
        fs::create_dir_all(self.forge_dir.join("plans")).await?;
        fs::create_dir_all(self.forge_dir.join("templates")).await?;
        self.ensure_gitignore().await?;
        Ok(())
    }

    /// Check if forge is initialized
    pub fn is_initialized(&self) -> bool {
        self.forge_dir.exists() && self.state_file.exists()
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
        let json = serde_json::to_string_pretty(state)
            .map_err(|e| ForgeError::State(e.to_string()))?;
        let tmp = self.state_file.with_extension("json.tmp");
        fs::write(&tmp, &json).await?;
        fs::rename(&tmp, &self.state_file).await?;
        Ok(())
    }

    /// Save agent output to .forge/agents/{id}/
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
            .forge_dir
            .parent()
            .expect("forge_dir has a parent")
            .join(".gitignore");

        if gitignore.exists() {
            let content = fs::read_to_string(&gitignore).await?;
            if !content.contains(".forge/") {
                let mut file = fs::OpenOptions::new()
                    .append(true)
                    .open(&gitignore)
                    .await?;
                file.write_all(b"\n# Forge agent orchestrator\n.forge/\n")
                    .await?;
            }
        } else {
            fs::write(&gitignore, "# Forge agent orchestrator\n.forge/\n").await?;
        }
        Ok(())
    }
}
