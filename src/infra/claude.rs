use crate::domain::agent::AgentResult;
use crate::error::ForgeError;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::process::Command;

/// Parsed JSON output from `claude -p --output-format json`
#[derive(Debug, Serialize, Deserialize)]
pub struct ClaudeJsonOutput {
    #[serde(rename = "type")]
    pub output_type: String,
    pub subtype: String,
    pub is_error: bool,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub num_turns: u32,
    #[serde(default)]
    pub result: String,
    #[serde(default)]
    pub session_id: String,
}

/// Build the command string to start an interactive claude session in a tmux pane.
/// Returns the shell command string to send via `tmux send-keys`.
pub fn interactive_command(
    system_prompt: Option<&str>,
    allowed_tools: &[String],
    disallowed_tools: &[String],
    permission_mode: Option<&str>,
    resume_session: Option<&str>,
    extra_args: &[String],
) -> String {
    let mut parts = vec!["claude".to_string()];

    if let Some(session_id) = resume_session {
        parts.push("--resume".to_string());
        parts.push(session_id.to_string());
    }

    if let Some(sp) = system_prompt {
        // Escape single quotes for shell
        let escaped = sp.replace('\'', "'\\''");
        parts.push("--system-prompt".to_string());
        parts.push(format!("'{escaped}'"));
    }

    if !allowed_tools.is_empty() {
        parts.push("--allowedTools".to_string());
        parts.push(format!("'{}'", allowed_tools.join(",")));
    }

    if !disallowed_tools.is_empty() {
        parts.push("--disallowedTools".to_string());
        parts.push(format!("'{}'", disallowed_tools.join(",")));
    }

    if let Some(pm) = permission_mode {
        parts.push("--permission-mode".to_string());
        parts.push(pm.to_string());
    }

    for arg in extra_args {
        parts.push(arg.clone());
    }

    parts.join(" ")
}

/// Run a headless claude agent, capturing JSON output.
pub async fn run_headless(
    prompt: &str,
    working_dir: &Path,
    system_prompt: Option<&str>,
    allowed_tools: &[String],
    disallowed_tools: &[String],
    permission_mode: Option<&str>,
    extra_args: &[String],
) -> Result<ClaudeJsonOutput, ForgeError> {
    let mut cmd = Command::new("claude");
    cmd.current_dir(working_dir);
    cmd.arg("-p");
    cmd.arg("--output-format").arg("json");

    if let Some(sp) = system_prompt {
        cmd.arg("--system-prompt").arg(sp);
    }
    if !allowed_tools.is_empty() {
        cmd.arg("--allowedTools").arg(allowed_tools.join(","));
    }
    if !disallowed_tools.is_empty() {
        cmd.arg("--disallowedTools").arg(disallowed_tools.join(","));
    }
    if let Some(pm) = permission_mode {
        cmd.arg("--permission-mode").arg(pm);
    }
    for arg in extra_args {
        cmd.arg(arg);
    }

    cmd.arg(prompt);

    let output = cmd.output().await?;

    if !output.status.success() && output.stdout.is_empty() {
        return Err(ForgeError::Claude(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: ClaudeJsonOutput = serde_json::from_str(&stdout).map_err(|e| {
        ForgeError::Claude(format!("Failed to parse claude output: {e}\nRaw: {stdout}"))
    })?;

    Ok(parsed)
}

/// Convert ClaudeJsonOutput to domain AgentResult
pub fn to_agent_result(output: &ClaudeJsonOutput) -> AgentResult {
    AgentResult {
        success: !output.is_error && output.subtype == "success",
        summary: truncate_summary(&output.result, 500),
        duration_ms: output.duration_ms,
        session_id: output.session_id.clone(),
        raw_result: Some(output.result.clone()),
    }
}

/// Check if claude CLI is available
pub fn is_available() -> bool {
    which::which("claude").is_ok()
}

fn truncate_summary(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        text.to_string()
    } else {
        format!("{}...", &text[..max_chars])
    }
}
