use crate::error::ForgeError;
use std::path::Path;
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

    /// Attach to a tmux session (replaces current terminal).
    /// Uses spawn_blocking to avoid blocking the tokio runtime — the
    /// attach-session command takes over the terminal until the user detaches.
    pub async fn attach(session_name: &str) -> Result<(), ForgeError> {
        let session = session_name.to_string();
        let status = tokio::task::spawn_blocking(move || {
            std::process::Command::new("tmux")
                .args(["attach-session", "-t", &session])
                .status()
        })
        .await
        .map_err(|e| ForgeError::Tmux(format!("attach task panicked: {e}")))?
        .map_err(ForgeError::from)?;

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

    /// Set escape-time so multi-byte CSI sequences (like \e[29~) arrive
    /// intact even with terminal input buffering (e.g. Warp) or SSH latency.
    /// Default 100ms is imperceptible for bare Escape but gives enough headroom
    /// to avoid sequence splitting that causes bindings to act as Escape.
    pub async fn set_escape_time(ms: u32) -> Result<(), ForgeError> {
        run_tmux(&["set-option", "-s", "escape-time", &ms.to_string()]).await
    }

    /// Enable extended key parsing so tmux correctly handles CSI u sequences
    /// (e.g. Shift+Enter) from modern terminals like Warp instead of
    /// misinterpreting them as copy-mode triggers.
    pub async fn enable_extended_keys() -> Result<(), ForgeError> {
        // Append extkeys to terminal-features so tmux parses CSI u input
        run_tmux(&[
            "set-option", "-as", "terminal-features", ",xterm*:extkeys",
        ]).await?;
        // Also set extended-keys so tmux forwards them to applications
        run_tmux(&["set-option", "-s", "extended-keys", "on"]).await
    }

    /// Set up navigation bindings using tmux user-keys for raw CSI sequences.
    ///
    /// `dashboard_key` and `overview_key` are CSI suffixes like "[29~" and "[33~".
    /// We register them as user-keys so tmux matches the raw byte sequences
    /// regardless of terminfo — this is the most reliable binding method.
    ///
    /// All 6 commands are batched into a single `tmux source-file` call so
    /// bindings transition atomically — there is never a moment where user-keys
    /// are defined but bind-key entries are missing, or vice versa.
    pub async fn setup_nav_bindings(
        tmux_session: &str,
        dashboard_key: &str,
        overview_key: &str,
        workspace_root: Option<&Path>,
    ) -> Result<(), ForgeError> {
        // Use \033 notation — tmux config file format for ESC byte.
        let dashboard_seq = format!("\\033{dashboard_key}");
        let overview_seq = format!("\\033{overview_key}");

        // Condition: window is NOT "dashboard" AND session IS the forge session
        let condition = format!(
            "#{{&&:#{{!=:#{{window_name}},dashboard}},#{{==:#{{session_name}},{tmux_session}}}}}"
        );

        // Build all commands as a single tmux config file.
        // tmux source-file processes these atomically in one server round-trip.
        let config = format!(
            r#"set-option -s user-keys[0] "{dashboard_seq}"
set-option -s user-keys[1] "{overview_seq}"
bind-key -n User0 if-shell -F '{condition}' 'select-window -t :dashboard' 'send-keys Escape'
bind-key -n User1 if-shell -F '{condition}' 'select-window -t :dashboard ; send-keys §' 'send-keys §'
bind-key d if-shell -F '{condition}' 'select-window -t :dashboard' 'send-keys Escape'
bind-key o if-shell -F '{condition}' 'select-window -t :dashboard ; send-keys §' 'send-keys §'
"#,
        );

        let tmp_path = std::env::temp_dir().join(format!("vibe-nav-{}.conf", std::process::id()));
        tokio::fs::write(&tmp_path, config.as_bytes()).await?;

        let result = run_tmux(&[
            "source-file",
            tmp_path.to_str().unwrap_or("/tmp/vibe-nav.conf"),
        ])
        .await;

        // Best-effort cleanup of temp file
        let _ = tokio::fs::remove_file(&tmp_path).await;

        result?;

        // Write PID lock so cleanup knows which instance owns the bindings
        if let Some(root) = workspace_root {
            Self::write_nav_lock(root).await;
        }

        Ok(())
    }

    /// Enable mouse scrolling and set scrollback buffer for a session.
    /// Hold Shift while clicking/dragging to use native terminal selection
    /// (bypasses tmux mouse capture for copy/paste).
    pub async fn configure_scrollback(session_name: &str) -> Result<(), ForgeError> {
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

    /// Remove the user-key navigation bindings and unset the user-keys options.
    /// Only cleans up if the PID lock belongs to this process (or no lock exists).
    /// This prevents one instance's exit from destroying another's bindings.
    /// All 6 cleanup commands are batched via `tmux source-file` for atomicity.
    pub async fn cleanup_nav_bindings(workspace_root: Option<&Path>) -> Result<(), ForgeError> {
        // Check PID lock — only clean up if we own the bindings
        if let Some(root) = workspace_root {
            let lock_path = root.join(".vibe").join("nav_bindings.lock");
            if let Ok(contents) = tokio::fs::read_to_string(&lock_path).await {
                if let Ok(pid) = contents.trim().parse::<u32>() {
                    if pid != std::process::id() {
                        // Another instance owns the bindings — don't touch them
                        debug!(our_pid = std::process::id(), lock_pid = pid, "skipping nav cleanup — another instance owns bindings");
                        return Ok(());
                    }
                }
            }
            // Remove the lock file since we're cleaning up
            let _ = tokio::fs::remove_file(&lock_path).await;
        }

        let config = r#"unbind-key -n User0
unbind-key -n User1
set-option -su user-keys[0]
set-option -su user-keys[1]
unbind-key d
unbind-key o
"#;

        let tmp_path = std::env::temp_dir().join(format!(
            "vibe-nav-cleanup-{}.conf",
            std::process::id()
        ));
        let _ = tokio::fs::write(&tmp_path, config.as_bytes()).await;
        let _ = run_tmux(&[
            "source-file",
            tmp_path.to_str().unwrap_or("/tmp/vibe-nav-cleanup.conf"),
        ])
        .await;
        let _ = tokio::fs::remove_file(&tmp_path).await;

        Ok(())
    }

    /// Deep verification of navigation bindings.
    /// Checks that user-key values match expected sequences AND that bind-key
    /// entries for User0/User1 actually exist. Returns true only if everything
    /// is correctly configured.
    ///
    /// Uses 2 tmux commands (down from 4) by fetching all server options in
    /// one call and parsing both user-keys values from the output.
    pub async fn verify_nav_bindings(
        dashboard_key: &str,
        overview_key: &str,
    ) -> bool {
        // 1. Single show-options call — check both user-keys values
        let opts = match run_tmux_output(&["show-options", "-s"]).await {
            Ok(output) => output,
            Err(_) => return false,
        };

        // Match against the CSI suffix (e.g. "[29~") in the output line.
        // tmux may display the raw ESC byte differently across versions,
        // so matching the suffix is more robust than exact byte comparison.
        let uk0_ok = opts
            .lines()
            .any(|l| l.starts_with("user-keys[0]") && l.contains(dashboard_key));
        let uk1_ok = opts
            .lines()
            .any(|l| l.starts_with("user-keys[1]") && l.contains(overview_key));

        if !uk0_ok || !uk1_ok {
            return false;
        }

        // 2. Single list-keys call — check bind-key entries
        let bindings = run_tmux_output(&["list-keys"]).await.unwrap_or_default();
        bindings.contains("User0") && bindings.contains("User1")
    }

    /// Write PID lock file so cleanup knows which process owns the bindings.
    async fn write_nav_lock(workspace_root: &Path) {
        let lock_path = workspace_root.join(".vibe").join("nav_bindings.lock");
        let pid = std::process::id().to_string();
        if let Err(e) = tokio::fs::write(&lock_path, pid.as_bytes()).await {
            debug!(error = %e, "failed to write nav lock file");
        }
    }

    /// Check if the PID in the nav lock file is still alive.
    /// Returns true if the lock exists with a dead PID (stale lock).
    pub async fn is_nav_lock_stale(workspace_root: &Path) -> bool {
        let lock_path = workspace_root.join(".vibe").join("nav_bindings.lock");
        match tokio::fs::read_to_string(&lock_path).await {
            Ok(contents) => {
                if let Ok(pid) = contents.trim().parse::<i32>() {
                    // Check if process is alive (signal 0 = existence check)
                    unsafe { libc::kill(pid, 0) != 0 }
                } else {
                    true // corrupt lock file
                }
            }
            Err(_) => false, // no lock file
        }
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

/// Check if an IO error is transient and worth retrying.
fn is_transient_io_error(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::Interrupted
    )
}

/// Check if a tmux stderr message indicates a transient failure.
fn is_transient_tmux_error(stderr: &str) -> bool {
    stderr.contains("server exited")
        || stderr.contains("lost server")
        || stderr.contains("no server running")
}

async fn run_tmux(args: &[&str]) -> Result<(), ForgeError> {
    let mut last_err = None;

    for attempt in 0..3u32 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(50 * 2u64.pow(attempt))).await;
        }

        match Command::new("tmux").args(args).output().await {
            Ok(output) => {
                if output.status.success() {
                    return Ok(());
                }
                let stderr = String::from_utf8_lossy(&output.stderr);
                // Ignore benign errors
                if stderr.contains("no server running") || stderr.contains("session not found") {
                    return Ok(());
                }
                // Retry on transient tmux errors
                if attempt < 2 && is_transient_tmux_error(&stderr) {
                    debug!(args = ?args, attempt, "tmux transient failure, retrying");
                    last_err = Some(stderr.to_string());
                    continue;
                }
                warn!(args = ?args, stderr = %stderr, "tmux command failed");
                return Err(ForgeError::Tmux(stderr.to_string()));
            }
            Err(e) => {
                if attempt < 2 && is_transient_io_error(&e) {
                    debug!(args = ?args, attempt, error = %e, "tmux IO error, retrying");
                    last_err = Some(e.to_string());
                    continue;
                }
                return Err(ForgeError::from(e));
            }
        }
    }

    Err(ForgeError::Tmux(format!(
        "tmux command failed after 3 attempts: {}",
        last_err.unwrap_or_default()
    )))
}

async fn run_tmux_output(args: &[&str]) -> Result<String, ForgeError> {
    let mut last_err = None;

    for attempt in 0..3u32 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(50 * 2u64.pow(attempt))).await;
        }

        match Command::new("tmux").args(args).output().await {
            Ok(output) => {
                if output.status.success() {
                    return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
                }
                let stderr = String::from_utf8_lossy(&output.stderr);
                // Retry on transient tmux errors
                if attempt < 2 && is_transient_tmux_error(&stderr) {
                    debug!(args = ?args, attempt, "tmux transient failure, retrying");
                    last_err = Some(stderr.to_string());
                    continue;
                }
                return Err(ForgeError::Tmux(stderr.to_string()));
            }
            Err(e) => {
                if attempt < 2 && is_transient_io_error(&e) {
                    debug!(args = ?args, attempt, error = %e, "tmux IO error, retrying");
                    last_err = Some(e.to_string());
                    continue;
                }
                return Err(ForgeError::from(e));
            }
        }
    }

    Err(ForgeError::Tmux(format!(
        "tmux command failed after 3 attempts: {}",
        last_err.unwrap_or_default()
    )))
}
