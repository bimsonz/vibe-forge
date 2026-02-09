use crate::error::VibeError;
use serde::Deserialize;
use std::path::Path;
use tokio::process::Command;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrInfo {
    pub number: u64,
    pub title: String,
    #[serde(default)]
    pub body: String,
    pub head_ref_name: String,
    pub base_ref_name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub additions: u64,
    #[serde(default)]
    pub deletions: u64,
    #[serde(default)]
    pub files: Vec<PrFile>,
}

#[derive(Debug, Deserialize)]
pub struct PrFile {
    pub path: String,
    #[serde(default)]
    pub additions: u64,
    #[serde(default)]
    pub deletions: u64,
}

/// Fetch PR metadata via gh CLI
pub async fn get_pr_info(pr_number: u64, repo_root: &Path) -> Result<PrInfo, VibeError> {
    let output = Command::new("gh")
        .current_dir(repo_root)
        .args([
            "pr",
            "view",
            &pr_number.to_string(),
            "--json",
            "number,title,body,headRefName,baseRefName,url,additions,deletions,files",
        ])
        .output()
        .await?;

    if !output.status.success() {
        return Err(VibeError::Git(format!(
            "gh pr view failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let info: PrInfo = serde_json::from_slice(&output.stdout)
        .map_err(|e| VibeError::Git(format!("Failed to parse PR info: {e}")))?;

    Ok(info)
}

/// Get the diff for a PR
pub async fn get_pr_diff(pr_number: u64, repo_root: &Path) -> Result<String, VibeError> {
    let output = Command::new("gh")
        .current_dir(repo_root)
        .args(["pr", "diff", &pr_number.to_string()])
        .output()
        .await?;

    if !output.status.success() {
        return Err(VibeError::Git(format!(
            "gh pr diff failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Get comments on a PR
pub async fn get_pr_comments(pr_number: u64, repo_root: &Path) -> Result<String, VibeError> {
    // Get review comments (inline code comments)
    let output = Command::new("gh")
        .current_dir(repo_root)
        .args([
            "pr",
            "view",
            &pr_number.to_string(),
            "--json",
            "comments,reviews",
        ])
        .output()
        .await?;

    if !output.status.success() {
        // Non-fatal — PR might have no comments
        return Ok(String::new());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Parse a PR identifier — could be a number or a URL
pub fn parse_pr_identifier(pr: &str) -> Result<u64, VibeError> {
    // Try direct number
    if let Ok(n) = pr.parse::<u64>() {
        return Ok(n);
    }

    // Try extracting from URL (e.g., https://github.com/org/repo/pull/123)
    if let Some(num_str) = pr.rsplit('/').next() {
        if let Ok(n) = num_str.parse::<u64>() {
            return Ok(n);
        }
    }

    Err(VibeError::User(format!(
        "Cannot parse PR identifier: '{pr}'. Use a PR number or URL."
    )))
}

/// Check if gh CLI is available
pub fn is_available() -> bool {
    which::which("gh").is_ok()
}
