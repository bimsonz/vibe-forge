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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::agent::AgentMode;

    #[test]
    fn test_parse_valid_template() {
        let content = r#"+++
description = "Test template"
mode = "headless"
permission_mode = "plan"
allowed_tools = ["Read", "Grep"]
disallowed_tools = ["Edit"]
+++

You are a test agent. Do test things."#;

        let t = AgentTemplate::parse("test", content).unwrap();
        assert_eq!(t.name, "test");
        assert_eq!(t.description, "Test template");
        assert_eq!(t.mode, AgentMode::Headless);
        assert_eq!(t.allowed_tools, vec!["Read", "Grep"]);
        assert_eq!(t.disallowed_tools, vec!["Edit"]);
        assert_eq!(t.permission_mode, Some("plan".into()));
        assert!(t.system_prompt.contains("test agent"));
    }

    #[test]
    fn test_parse_interactive_mode() {
        let content = r#"+++
description = "Interactive agent"
mode = "interactive"
+++

Do things interactively."#;

        let t = AgentTemplate::parse("interactive", content).unwrap();
        assert_eq!(t.mode, AgentMode::Interactive);
        assert!(t.allowed_tools.is_empty());
        assert!(t.disallowed_tools.is_empty());
        assert!(t.permission_mode.is_none());
    }

    #[test]
    fn test_parse_missing_frontmatter() {
        let content = "# No frontmatter here\nJust markdown.";
        assert!(AgentTemplate::parse("bad", content).is_err());
    }

    #[test]
    fn test_parse_invalid_toml() {
        let content = "+++\nnot = [valid toml\n+++\nBody";
        assert!(AgentTemplate::parse("bad", content).is_err());
    }

    #[test]
    fn test_all_builtins_parse() {
        for (name, content) in BUILTIN_TEMPLATES {
            let result = AgentTemplate::parse(name, content);
            assert!(result.is_ok(), "Built-in template '{name}' failed to parse: {:?}", result.err());
        }
    }

    #[test]
    fn test_load_all_includes_builtins() {
        let empty_dirs: &[&str] = &[];
        let templates = AgentTemplate::load_all(empty_dirs);
        assert_eq!(templates.len(), 5);
        let names: Vec<_> = templates.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"planner"));
        assert!(names.contains(&"implementer"));
        assert!(names.contains(&"reviewer"));
        assert!(names.contains(&"tester"));
        assert!(names.contains(&"refactorer"));
    }

    #[test]
    fn test_load_by_name_builtin() {
        let empty_dirs: &[&str] = &[];
        let t = AgentTemplate::load("reviewer", empty_dirs).unwrap();
        assert_eq!(t.name, "reviewer");
        assert!(!t.system_prompt.is_empty());
    }

    #[test]
    fn test_load_by_name_not_found() {
        let empty_dirs: &[&str] = &[];
        assert!(AgentTemplate::load("nonexistent", empty_dirs).is_err());
    }
}
