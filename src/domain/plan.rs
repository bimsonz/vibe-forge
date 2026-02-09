use crate::error::VibeError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// A Plan is a shared document between agents.
/// Lives at .vibe/plans/{id}.md with TOML frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: Uuid,
    pub title: String,
    pub session_name: Option<String>,
    pub file_path: PathBuf,
    pub status: PlanStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PlanStatus {
    Draft,
    Active,
    Completed,
    Superseded,
}

impl std::fmt::Display for PlanStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Draft => write!(f, "Draft"),
            Self::Active => write!(f, "Active"),
            Self::Completed => write!(f, "Completed"),
            Self::Superseded => write!(f, "Superseded"),
        }
    }
}

/// TOML frontmatter stored in plan files.
#[derive(Debug, Serialize, Deserialize)]
struct PlanFrontmatter {
    id: Uuid,
    title: String,
    #[serde(default)]
    session_name: Option<String>,
    #[serde(default = "default_status")]
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn default_status() -> String {
    "Draft".into()
}

impl Plan {
    pub fn new(title: String, session_name: Option<String>, plans_dir: &Path) -> Self {
        let id = Uuid::new_v4();
        let slug = slug_from_title(&title);
        let file_path = plans_dir.join(format!("{slug}-{}.md", &id.to_string()[..8]));
        Self {
            id,
            title,
            session_name,
            file_path,
            status: PlanStatus::Draft,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// Write the plan to disk as markdown with TOML frontmatter.
    pub fn save(&self, body: &str) -> Result<(), VibeError> {
        let frontmatter = PlanFrontmatter {
            id: self.id,
            title: self.title.clone(),
            session_name: self.session_name.clone(),
            status: self.status.to_string(),
            created_at: self.created_at,
            updated_at: Utc::now(),
        };
        let toml_str =
            toml::to_string_pretty(&frontmatter).map_err(|e| VibeError::State(e.to_string()))?;
        let content = format!("+++\n{toml_str}+++\n\n{body}");

        if let Some(parent) = self.file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.file_path, content)?;
        Ok(())
    }

    /// Load a plan from a markdown file with TOML frontmatter.
    pub fn load(path: &Path) -> Result<(Self, String), VibeError> {
        let content = std::fs::read_to_string(path)?;
        let (plan, body) = Self::parse(&content, path)?;
        Ok((plan, body))
    }

    /// Parse plan content from a string.
    fn parse(content: &str, path: &Path) -> Result<(Self, String), VibeError> {
        let content = content.trim_start();
        if !content.starts_with("+++") {
            return Err(VibeError::State(format!(
                "Plan file missing frontmatter: {}",
                path.display()
            )));
        }

        let after_open = &content[3..];
        let close_pos = after_open.find("+++").ok_or_else(|| {
            VibeError::State(format!(
                "Plan file missing closing +++: {}",
                path.display()
            ))
        })?;

        let toml_str = &after_open[..close_pos];
        let body = after_open[close_pos + 3..].trim_start().to_string();

        let fm: PlanFrontmatter =
            toml::from_str(toml_str).map_err(|e| VibeError::State(e.to_string()))?;

        let status = match fm.status.as_str() {
            "Active" => PlanStatus::Active,
            "Completed" => PlanStatus::Completed,
            "Superseded" => PlanStatus::Superseded,
            _ => PlanStatus::Draft,
        };

        let plan = Plan {
            id: fm.id,
            title: fm.title,
            session_name: fm.session_name,
            file_path: path.to_path_buf(),
            status,
            created_at: fm.created_at,
            updated_at: fm.updated_at,
        };

        Ok((plan, body))
    }

    /// Load all plans from a directory.
    pub fn load_all(plans_dir: &Path) -> Vec<Self> {
        let mut plans = Vec::new();
        if let Ok(entries) = std::fs::read_dir(plans_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "md") {
                    if let Ok((plan, _)) = Self::load(&path) {
                        plans.push(plan);
                    }
                }
            }
        }
        plans.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        plans
    }
}

/// Create a URL-friendly slug from a title.
fn slug_from_title(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_slug_from_title() {
        assert_eq!(slug_from_title("My Cool Plan"), "my-cool-plan");
        assert_eq!(slug_from_title("fix auth/login"), "fix-auth-login");
        assert_eq!(slug_from_title("  spaces  "), "spaces");
    }

    #[test]
    fn test_plan_roundtrip() {
        let dir = std::env::temp_dir().join("vibe-test-plans");
        let _ = std::fs::create_dir_all(&dir);

        let plan = Plan::new("Test Plan".into(), Some("my-session".into()), &dir);
        let body = "# Implementation\n\n- Step 1\n- Step 2\n";
        plan.save(body).unwrap();

        let (loaded, loaded_body) = Plan::load(&plan.file_path).unwrap();
        assert_eq!(loaded.id, plan.id);
        assert_eq!(loaded.title, "Test Plan");
        assert_eq!(loaded.session_name, Some("my-session".into()));
        assert_eq!(loaded.status, PlanStatus::Draft);
        assert_eq!(loaded_body, body);

        // Cleanup
        let _ = std::fs::remove_file(&plan.file_path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn test_parse_no_frontmatter() {
        let result = Plan::parse("# Just markdown", &PathBuf::from("test.md"));
        assert!(result.is_err());
    }
}
