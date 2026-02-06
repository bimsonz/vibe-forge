use crate::error::ForgeError;
use tokio::process::Command;
use tracing::{debug, warn};

/// All tmux operations. Shells out to `tmux` CLI.
pub struct TmuxController;

#[derive(Debug, Clone)]
pub struct PaneInfo {
    pub pane_id: String,
    pub pid: u32,
    pub command: String,
    pub window_name: String,
    pub current_path: String,
}

impl TmuxController {
    /// Check if tmux is installed
    pub fn is_available() -> bool {
        which::which("tmux").is_ok()
    }

    /// Check if a tmux session exists
    pub async fn session_exists(session_name: &str) -> Result<bool, ForgeError> {
        let output = Command::new("tmux")
            .args(["has-session", "-t", session_name])
            .output()
            .await?;
        Ok(output.status.success())
    }

    /// Create the forge tmux session if it doesn't exist
    pub async fn ensure_session(session_name: &str) -> Result<(), ForgeError> {
        if Self::session_exists(session_name).await? {
            debug!(session = session_name, "tmux session already exists");
            return Ok(());
        }
        debug!(session = session_name, "creating tmux session");
        run_tmux(&["new-session", "-d", "-s", session_name, "-x", "200", "-y", "50"]).await
    }

    /// Create a new window within the forge session
    pub async fn create_window(
        session_name: &str,
        window_name: &str,
        working_dir: &str,
    ) -> Result<String, ForgeError> {
        run_tmux_output(&[
            "new-window",
            "-t",
            session_name,
            "-n",
            window_name,
            "-c",
            working_dir,
            "-P",
            "-F",
            "#{window_id}",
        ])
        .await
    }

    /// Split an existing window to create a pane
    pub async fn split_pane(
        target_window: &str,
        working_dir: &str,
        horizontal: bool,
    ) -> Result<String, ForgeError> {
        let split_flag = if horizontal { "-h" } else { "-v" };
        run_tmux_output(&[
            "split-window",
            "-t",
            target_window,
            split_flag,
            "-c",
            working_dir,
            "-P",
            "-F",
            "#{pane_id}",
        ])
        .await
    }

    /// Send a command string to a tmux pane
    pub async fn send_keys(pane_id: &str, command: &str) -> Result<(), ForgeError> {
        run_tmux(&["send-keys", "-t", pane_id, command, "Enter"]).await
    }

    /// Capture the current contents of a pane
    pub async fn capture_pane(pane_id: &str, lines: u32) -> Result<String, ForgeError> {
        let start = format!("-{lines}");
        run_tmux_output(&["capture-pane", "-t", pane_id, "-p", "-S", &start]).await
    }

    /// Kill a tmux window
    pub async fn kill_window(target: &str) -> Result<(), ForgeError> {
        run_tmux(&["kill-window", "-t", target]).await
    }

    /// Select a specific pane within a window
    pub async fn select_pane(pane_id: &str) -> Result<(), ForgeError> {
        run_tmux(&["select-pane", "-t", pane_id]).await
    }

    /// Kill a tmux pane
    pub async fn kill_pane(pane_id: &str) -> Result<(), ForgeError> {
        run_tmux(&["kill-pane", "-t", pane_id]).await
    }

    /// Get the name of the currently active window
    pub async fn current_window_name() -> Result<String, ForgeError> {
        run_tmux_output(&["display-message", "-p", "#{window_name}"]).await
    }

    /// Get the first pane ID for a window
    pub async fn first_pane_id(target_window: &str) -> Result<String, ForgeError> {
        run_tmux_output(&[
            "list-panes",
            "-t",
            target_window,
            "-F",
            "#{pane_id}",
        ])
        .await
        .map(|output| {
            output.lines().next().unwrap_or("").to_string()
        })
    }

    /// Kill an entire tmux session
    pub async fn kill_session(session_name: &str) -> Result<(), ForgeError> {
        run_tmux(&["kill-session", "-t", session_name]).await
    }

    /// List all panes in a session
    pub async fn list_panes(session_name: &str) -> Result<Vec<PaneInfo>, ForgeError> {
        let output = run_tmux_output(&[
            "list-panes",
            "-s",
            "-t",
            session_name,
            "-F",
            "#{pane_id}\t#{pane_pid}\t#{pane_current_command}\t#{window_name}\t#{pane_current_path}",
        ])
        .await?;

        let panes = output
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() >= 5 {
                    Some(PaneInfo {
                        pane_id: parts[0].to_string(),
                        pid: parts[1].parse().unwrap_or(0),
                        command: parts[2].to_string(),
                        window_name: parts[3].to_string(),
                        current_path: parts[4].to_string(),
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(panes)
    }

    /// Set a pane title
    pub async fn set_pane_title(pane_id: &str, title: &str) -> Result<(), ForgeError> {
        run_tmux(&["select-pane", "-t", pane_id, "-T", title]).await
    }

    /// Attach to a tmux session (replaces current terminal)
    pub async fn attach(session_name: &str) -> Result<(), ForgeError> {
        // This is special — it replaces our process
        let status = std::process::Command::new("tmux")
            .args(["attach-session", "-t", session_name])
            .status()?;

        if !status.success() {
            return Err(ForgeError::Tmux("Failed to attach to session".into()));
        }
        Ok(())
    }

    /// Select a specific window within the session
    pub async fn select_window(target: &str) -> Result<(), ForgeError> {
        run_tmux(&["select-window", "-t", target]).await
    }

    /// Rename the current tmux window
    pub async fn rename_window(name: &str) -> Result<(), ForgeError> {
        run_tmux(&["rename-window", name]).await
    }

    /// Disable automatic window renaming for a specific window.
    /// Prevents tmux from renaming windows when the running command changes
    /// (e.g., "my-session" → "node" when Claude starts).
    pub async fn disable_auto_rename_for(target: &str) -> Result<(), ForgeError> {
        run_tmux(&["set-option", "-w", "-t", target, "automatic-rename", "off"]).await?;
        run_tmux(&["set-option", "-w", "-t", target, "allow-rename", "off"]).await
    }

    /// Set up navigation bindings to return to dashboard from session windows.
    /// Binds both § and Escape. Escape is safe with escape-time ≤ 50ms since
    /// terminal escape sequences (arrows, mouse) complete in <1ms.
    pub async fn setup_nav_bindings(forge_session: &str) -> Result<(), ForgeError> {
        // Condition: window is NOT "dashboard" AND session IS the forge session
        let mut condition =
            String::from("#{&&:#{!=:#{window_name},dashboard},#{==:#{session_name},");
        condition.push_str(forge_session);
        condition.push_str("}}");

        // § key: if in a session window, switch to dashboard AND trigger overview;
        // otherwise pass through so the TUI handles it directly.
        run_tmux(&[
            "bind-key",
            "-n",
            "§",
            "if-shell",
            "-F",
            &condition,
            "select-window -t :dashboard ; send-keys §",
            "send-keys §",
        ])
        .await?;

        // Escape key: same behavior
        run_tmux(&[
            "bind-key",
            "-n",
            "Escape",
            "if-shell",
            "-F",
            &condition,
            "select-window -t :dashboard",
            "send-keys Escape",
        ])
        .await?;

        Ok(())
    }

    /// Enable mouse support and set scrollback buffer for a session.
    /// Allows mouse wheel scrolling through tmux pane history.
    pub async fn enable_mouse(session_name: &str) -> Result<(), ForgeError> {
        run_tmux(&["set-option", "-t", session_name, "mouse", "on"]).await?;
        run_tmux(&["set-option", "-t", session_name, "history-limit", "10000"]).await
    }

    /// Hide the tmux status bar for a session
    pub async fn hide_status_bar(session_name: &str) -> Result<(), ForgeError> {
        run_tmux(&["set-option", "-t", session_name, "status", "off"]).await
    }

    /// Show the tmux status bar for a session
    pub async fn show_status_bar(session_name: &str) -> Result<(), ForgeError> {
        run_tmux(&["set-option", "-t", session_name, "status", "on"]).await
    }

    /// Remove the navigation bindings (§ and Escape)
    pub async fn cleanup_nav_bindings() -> Result<(), ForgeError> {
        let _ = run_tmux(&["unbind-key", "-n", "§"]).await;
        let _ = run_tmux(&["unbind-key", "-n", "Escape"]).await;
        Ok(())
    }

    /// Get the name of the tmux session we're currently inside
    pub async fn current_session_name() -> Result<String, ForgeError> {
        run_tmux_output(&["display-message", "-p", "#{session_name}"]).await
    }

    /// Switch the current tmux client to a different session
    pub async fn switch_client(session_name: &str) -> Result<(), ForgeError> {
        run_tmux(&["switch-client", "-t", session_name]).await
    }

    /// Detach the current client from the tmux session without killing the session
    pub async fn detach_client() -> Result<(), ForgeError> {
        run_tmux(&["detach-client"]).await
    }

    /// Get the current command running in a pane (first pane of target window)
    pub async fn pane_current_command(target: &str) -> Result<String, ForgeError> {
        run_tmux_output(&["display-message", "-t", target, "-p", "#{pane_current_command}"])
            .await
    }

    /// Get the current pane ID
    pub async fn current_pane_id() -> Result<String, ForgeError> {
        run_tmux_output(&["display-message", "-p", "#{pane_id}"]).await
    }

    /// Check if a window exists (without switching to it)
    pub async fn window_exists(target: &str) -> bool {
        run_tmux_output(&["display-message", "-t", target, "-p", "#{window_id}"])
            .await
            .is_ok()
    }

    /// Check if a pane exists
    pub async fn pane_exists(pane_id: &str) -> bool {
        run_tmux(&["display-message", "-t", pane_id, "-p", "#{pane_id}"])
            .await
            .is_ok()
    }

    /// Get the window ID containing a pane
    pub async fn window_id_for_pane(pane_id: &str) -> Result<String, ForgeError> {
        run_tmux_output(&["display-message", "-t", pane_id, "-p", "#{window_id}"]).await
    }

}

async fn run_tmux(args: &[&str]) -> Result<(), ForgeError> {
    let output = Command::new("tmux").args(args).output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Ignore "no server running" errors for has-session checks
        if !stderr.contains("no server running") && !stderr.contains("session not found") {
            warn!(args = ?args, stderr = %stderr, "tmux command failed");
            return Err(ForgeError::Tmux(stderr.to_string()));
        }
    }
    Ok(())
}

async fn run_tmux_output(args: &[&str]) -> Result<String, ForgeError> {
    let output = Command::new("tmux").args(args).output().await?;

    if !output.status.success() {
        return Err(ForgeError::Tmux(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
