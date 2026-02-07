use crate::error::ForgeError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Global config: ~/.config/vibe/config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GlobalConfig {
    pub tmux_session_prefix: String,
    pub worktree_suffix: String,
    pub claude_extra_args: Vec<String>,
    pub template_dirs: Vec<PathBuf>,
    pub clipboard_on_complete: bool,
    pub notify_on_complete: bool,
    pub max_concurrent_agents: Option<usize>,
    /// CSI sequence suffix for "back to dashboard" binding (default: "[29~")
    /// The full escape sequence sent by the terminal is \e + this suffix.
    pub dashboard_key: String,
    /// CSI sequence suffix for "overview" binding (default: "[33~")
    pub overview_key: String,
    /// tmux escape-time in milliseconds (default: 100).
    /// Higher values improve reliability over SSH at the cost of Escape key latency.
    pub escape_time_ms: u32,
}

impl GlobalConfig {
    /// Human-readable label for the dashboard key (for status bar hints).
    pub fn dashboard_key_display(&self) -> String {
        csi_display_name(&self.dashboard_key)
    }

    /// Human-readable label for the overview key (for status bar hints).
    pub fn overview_key_display(&self) -> String {
        csi_display_name(&self.overview_key)
    }

    /// Check if a crossterm KeyCode matches the configured overview key.
    pub fn matches_overview_key(&self, code: &crossterm::event::KeyCode) -> bool {
        csi_to_keycode(&self.overview_key)
            .is_some_and(|expected| std::mem::discriminant(&expected) == std::mem::discriminant(code) && expected == *code)
    }
}

/// Map a CSI suffix like "[29~" to the crossterm KeyCode it produces.
fn csi_to_keycode(suffix: &str) -> Option<crossterm::event::KeyCode> {
    use crossterm::event::KeyCode;
    // CSI sequences of the form \e[N~ map to function keys
    let inner = suffix.strip_prefix('[')?.strip_suffix('~')?;
    let n: u8 = inner.parse().ok()?;
    // Standard xterm mapping: 11-15→F1-F5, 17-21→F6-F10, 23-24→F11-F12,
    // 25-26→F13-F14, 28-29→F15-F16, 31-34→F17-F20
    let fkey = match n {
        11..=15 => n - 10,        // F1-F5
        17..=21 => n - 11,        // F6-F10
        23..=24 => n - 12,        // F11-F12
        25..=26 => n - 12,        // F13-F14
        28..=29 => n - 13,        // F15-F16
        31..=34 => n - 14,        // F17-F20
        _ => return None,
    };
    Some(KeyCode::F(fkey))
}

/// Human-readable name for a CSI suffix.
fn csi_display_name(suffix: &str) -> String {
    if let Some(kc) = csi_to_keycode(suffix) {
        if let crossterm::event::KeyCode::F(n) = kc {
            return format!("F{n}");
        }
    }
    suffix.to_string()
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            tmux_session_prefix: "vibe-".into(),
            worktree_suffix: "-vibe-".into(),
            claude_extra_args: vec![],
            template_dirs: vec![],
            clipboard_on_complete: true,
            notify_on_complete: true,
            max_concurrent_agents: None,
            dashboard_key: "[29~".into(),
            overview_key: "[33~".into(),
            escape_time_ms: 100,
        }
    }
}

/// Workspace config: .vibe/config.toml (project-specific overrides)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceConfig {
    pub worktree_base_dir: Option<PathBuf>,
    pub default_branch: Option<String>,
    pub template_dir: Option<PathBuf>,
    pub pre_session_hook: Option<String>,
    pub post_session_hook: Option<String>,
}

/// Merged config with resolved values
#[derive(Debug, Clone)]
pub struct MergedConfig {
    pub global: GlobalConfig,
    pub workspace: WorkspaceConfig,
    pub global_config_dir: PathBuf,
}

impl MergedConfig {
    pub fn tmux_session_prefix(&self) -> &str {
        &self.global.tmux_session_prefix
    }

    pub fn worktree_base_dir(&self, workspace_root: &Path) -> PathBuf {
        self.workspace
            .worktree_base_dir
            .clone()
            .unwrap_or_else(|| {
                workspace_root
                    .parent()
                    .unwrap_or(workspace_root)
                    .to_path_buf()
            })
    }

    pub fn template_dirs(&self, workspace_root: &Path) -> Vec<PathBuf> {
        let mut dirs = vec![];

        // Workspace templates (highest priority)
        let ws_templates = workspace_root.join(".vibe").join("templates");
        if ws_templates.exists() {
            dirs.push(ws_templates);
        }
        if let Some(ref dir) = self.workspace.template_dir {
            if dir.exists() {
                dirs.push(dir.clone());
            }
        }

        // User global templates
        let global_templates = self.global_config_dir.join("templates");
        if global_templates.exists() {
            dirs.push(global_templates);
        }

        // Extra template dirs from global config
        for dir in &self.global.template_dirs {
            if dir.exists() {
                dirs.push(dir.clone());
            }
        }

        dirs
    }
}

/// Load and merge configuration from all sources.
///
/// Resolution order:
/// 1. .vibe/config.toml (workspace)
/// 2. ~/.config/vibe/config.toml (global)
/// 3. Built-in defaults
pub fn load_config(workspace_root: Option<&Path>) -> Result<MergedConfig, ForgeError> {
    let global_config_dir = global_config_dir();

    // Load global config
    let global_config_path = global_config_dir.join("config.toml");
    let global = if global_config_path.exists() {
        let content = std::fs::read_to_string(&global_config_path)
            .map_err(|e| ForgeError::Config(format!("Failed to read global config: {e}")))?;
        toml::from_str(&content)
            .map_err(|e| ForgeError::Config(format!("Failed to parse global config: {e}")))?
    } else {
        GlobalConfig::default()
    };

    // Load workspace config
    let workspace = if let Some(root) = workspace_root {
        let ws_config_path = root.join(".vibe").join("config.toml");
        if ws_config_path.exists() {
            let content = std::fs::read_to_string(&ws_config_path)
                .map_err(|e| ForgeError::Config(format!("Failed to read workspace config: {e}")))?;
            toml::from_str(&content)
                .map_err(|e| ForgeError::Config(format!("Failed to parse workspace config: {e}")))?
        } else {
            WorkspaceConfig::default()
        }
    } else {
        WorkspaceConfig::default()
    };

    Ok(MergedConfig {
        global,
        workspace,
        global_config_dir,
    })
}

pub fn global_config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("vibe")
}

/// Ensure global config directory exists
pub fn ensure_global_config_dir() -> Result<PathBuf, ForgeError> {
    let dir = global_config_dir();
    std::fs::create_dir_all(&dir)?;
    std::fs::create_dir_all(dir.join("templates"))?;
    Ok(dir)
}
