use crate::domain::agent::AgentMode;
use crate::error::ForgeError;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

/// Agent template parsed from a markdown file with TOML +++ frontmatter.
#[derive(Debug, Clone)]
pub struct AgentTemplate {
    pub name: String,
    pub description: String,
    pub mode: AgentMode,
    pub system_prompt: String,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    pub permission_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TemplateFrontmatter {
    description: String,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    permission_mode: Option<String>,
    #[serde(default)]
    allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    disallowed_tools: Option<Vec<String>>,
}

impl AgentTemplate {
    /// Parse a template from markdown content with +++ TOML frontmatter.
    pub fn parse(name: &str, content: &str) -> Result<Self, ForgeError> {
        let parts: Vec<&str> = content.splitn(3, "+++").collect();
        if parts.len() != 3 {
            return Err(ForgeError::Template(format!(
                "Template '{name}' missing +++ frontmatter delimiters"
            )));
        }

        let frontmatter: TemplateFrontmatter = toml::from_str(parts[1].trim())
            .map_err(|e| ForgeError::Template(format!("Template '{name}' frontmatter error: {e}")))?;

        let system_prompt = parts[2].trim().to_string();

        Ok(Self {
            name: name.to_string(),
            description: frontmatter.description,
            mode: match frontmatter.mode.as_deref() {
                Some("interactive") => AgentMode::Interactive,
                _ => AgentMode::Headless,
            },
            system_prompt,
            allowed_tools: frontmatter.allowed_tools.unwrap_or_default(),
            disallowed_tools: frontmatter.disallowed_tools.unwrap_or_default(),
            permission_mode: frontmatter.permission_mode,
        })
    }

    /// Load all templates from search paths + built-ins.
    /// Resolution order: workspace > user global > built-in.
    pub fn load_all(template_dirs: &[impl AsRef<Path>]) -> Vec<Self> {
        let mut templates = Vec::new();
        let mut seen_names = HashSet::new();

        // Load from directories (workspace and user global, in priority order)
        for dir in template_dirs {
            load_from_dir(dir.as_ref(), &mut templates, &mut seen_names);
        }

        // Built-in templates (lowest priority)
        for (name, content) in BUILTIN_TEMPLATES {
            if !seen_names.contains(*name) {
                if let Ok(t) = Self::parse(name, content) {
                    seen_names.insert(name.to_string());
                    templates.push(t);
                }
            }
        }

        templates
    }

    /// Load a single template by name from search paths + built-ins.
    pub fn load(name: &str, template_dirs: &[impl AsRef<Path>]) -> Result<Self, ForgeError> {
        // Check directories first
        for dir in template_dirs {
            let path = dir.as_ref().join(format!("{name}.md"));
            if path.exists() {
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| ForgeError::Template(format!("Failed to read template: {e}")))?;
                return Self::parse(name, &content);
            }
        }

        // Check built-ins
        for (builtin_name, content) in BUILTIN_TEMPLATES {
            if *builtin_name == name {
                return Self::parse(name, content);
            }
        }

        Err(ForgeError::Template(format!("Template '{name}' not found")))
    }
}

fn load_from_dir(dir: &Path, templates: &mut Vec<AgentTemplate>, seen: &mut HashSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if !seen.contains(stem) {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(t) = AgentTemplate::parse(stem, &content) {
                            seen.insert(stem.to_string());
                            templates.push(t);
                        }
                    }
                }
            }
        }
    }
}

const BUILTIN_TEMPLATES: &[(&str, &str)] = &[
    ("planner", include_str!("../../templates/planner.md")),
    ("implementer", include_str!("../../templates/implementer.md")),
    ("reviewer", include_str!("../../templates/reviewer.md")),
    ("tester", include_str!("../../templates/tester.md")),
    ("refactorer", include_str!("../../templates/refactorer.md")),
];
