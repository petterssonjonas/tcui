#![allow(dead_code)]
use color_eyre::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Soul {
    pub name: String,
    pub soul_type: String,
    pub description: String,
    pub content: String,
}

impl Soul {
    pub fn load(name: &str) -> Result<Self> {
        let path = Self::soul_path(name)?;
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            Self::parse(&content)
        } else {
            Ok(Self::default_soul(name))
        }
    }

    fn parse(content: &str) -> Result<Self> {
        let parts: Vec<&str> = content.splitn(2, "---\n").collect();
        if parts.len() == 2 {
            let frontmatter = parts[1];
            let body = parts[0];

            let parsed_name =
                Self::extract_field(frontmatter, "name").unwrap_or_else(|| "default".to_string());
            let soul_type =
                Self::extract_field(frontmatter, "type").unwrap_or_else(|| "soul".to_string());
            let description = Self::extract_field(frontmatter, "description").unwrap_or_default();

            Ok(Self {
                name: parsed_name,
                soul_type,
                description,
                content: body.to_string(),
            })
        } else {
            Ok(Self {
                name: content.to_string(),
                soul_type: "soul".to_string(),
                description: String::new(),
                content: content.to_string(),
            })
        }
    }

    fn extract_field(frontmatter: &str, field: &str) -> Option<String> {
        let pattern = format!("{}:", field);
        frontmatter
            .lines()
            .find(|line| line.starts_with(&pattern))
            .map(|line| {
                line[line.find(':').map(|i| i + 1).unwrap_or(0)..]
                    .trim()
                    .to_string()
            })
    }

    fn soul_path(name: &str) -> Result<PathBuf> {
        let dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        Ok(dir.join("tcui").join("souls").join(format!("{}.md", name)))
    }

    fn default_soul(name: &str) -> Self {
        Self {
            name: name.to_string(),
            soul_type: "soul".to_string(),
            description: "Default assistant".to_string(),
            content: "You are a helpful assistant.".to_string(),
        }
    }
}

impl Default for Soul {
    fn default() -> Self {
        Self::default_soul("default")
    }
}
