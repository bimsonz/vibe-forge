use crate::domain::agent::AgentResult;
use crate::error::ForgeError;
use arboard::Clipboard;

/// Copy agent result summary to clipboard with formatting
pub fn copy_agent_result(result: &AgentResult, agent_name: &str) -> Result<(), ForgeError> {
    let text = format!(
        "## Agent: {}\n\n{}\n\n---\n*Duration: {:.1}s*",
        agent_name,
        result.raw_result.as_deref().unwrap_or(&result.summary),
        result.duration_ms as f64 / 1000.0,
    );
    copy_text(&text)
}

/// Copy arbitrary text to clipboard
pub fn copy_text(text: &str) -> Result<(), ForgeError> {
    let mut clipboard =
        Clipboard::new().map_err(|e| ForgeError::Clipboard(e.to_string()))?;
    clipboard
        .set_text(text)
        .map_err(|e| ForgeError::Clipboard(e.to_string()))?;
    Ok(())
}
