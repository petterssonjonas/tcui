use crate::config::McpServerConfig;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct McpCapabilities {
    pub tools: bool,
    pub resources: bool,
    pub prompts: bool,
}

impl McpCapabilities {
    pub const fn tools_only() -> Self {
        Self {
            tools: true,
            resources: false,
            prompts: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct McpProfile {
    pub name: &'static str,
    pub skill: &'static str,
    pub description: &'static str,
    pub program: &'static str,
    pub args: &'static [&'static str],
    pub api_key_env: Option<&'static str>,
    pub key_store_name: Option<&'static str>,
    pub vault_env: Option<&'static str>,
    pub capabilities: McpCapabilities,
}

impl McpProfile {
    pub fn config(self, enabled: bool) -> McpServerConfig {
        McpServerConfig {
            name: self.name.to_string(),
            transport: "stdio".to_string(),
            command: Some(self.program.to_string()),
            args: Some(self.args.iter().map(|arg| (*arg).to_string()).collect()),
            url: None,
            enabled,
        }
    }
}

const PROFILES: [McpProfile; 5] = [
    McpProfile {
        name: "Exa",
        skill: "exa",
        description: "Exa Search",
        program: "npx",
        args: &["-y", "exa-mcp-server@3.2.1"],
        api_key_env: Some("EXA_API_KEY"),
        key_store_name: Some("Exa Search"),
        vault_env: None,
        capabilities: McpCapabilities::tools_only(),
    },
    McpProfile {
        name: "Tavily",
        skill: "tavily",
        description: "Tavily Search",
        program: "npx",
        args: &["-y", "tavily-mcp@0.2.20"],
        api_key_env: Some("TAVILY_API_KEY"),
        key_store_name: Some("Tavily Search"),
        vault_env: None,
        capabilities: McpCapabilities::tools_only(),
    },
    McpProfile {
        name: "Firecrawl",
        skill: "firecrawl",
        description: "Firecrawl Search",
        program: "npx",
        args: &["-y", "firecrawl-mcp@3.22.1"],
        api_key_env: Some("FIRECRAWL_API_KEY"),
        key_store_name: Some("Firecrawl Search"),
        vault_env: None,
        capabilities: McpCapabilities::tools_only(),
    },
    McpProfile {
        name: "GNOME Desktop",
        skill: "gnome",
        description: "GNOME Desktop",
        program: "uvx",
        args: &["gnome-desktop-mcp==0.1.0"],
        api_key_env: None,
        key_store_name: None,
        vault_env: None,
        capabilities: McpCapabilities::tools_only(),
    },
    McpProfile {
        name: "Obsidian",
        skill: "obsidian",
        description: "Obsidian",
        program: "uvx",
        args: &[
            "--from",
            "git+https://github.com/Vasallo94/obsidian-mcp-server.git@ac9889f93c39c3a6a38515fdeb374e61f87c70a2",
            "obsidian-mcp-server",
        ],
        api_key_env: None,
        key_store_name: None,
        vault_env: Some("OBSIDIAN_VAULT_PATH"),
        capabilities: McpCapabilities {
            tools: true,
            resources: true,
            prompts: true,
        },
    },
];

pub fn profiles() -> &'static [McpProfile] {
    &PROFILES
}

pub fn profile_by_name(name: &str) -> Option<&'static McpProfile> {
    profiles()
        .iter()
        .find(|profile| normalize(profile.name) == normalize(name))
}

pub fn profile_by_skill(skill: &str) -> Option<&'static McpProfile> {
    profiles()
        .iter()
        .find(|profile| normalize(profile.skill) == normalize(skill))
}

pub fn lookup_profile(query: &str) -> Option<&'static McpProfile> {
    profile_by_skill(query).or_else(|| profile_by_name(query))
}

pub fn merged_configs(existing: &[McpServerConfig]) -> Vec<McpServerConfig> {
    let enabled_by_name = existing.iter().fold(HashMap::new(), |mut acc, row| {
        if let Some(profile) = lookup_profile(&row.name) {
            acc.insert(normalize(profile.name), row.enabled);
        }
        acc
    });

    let mut configs = Vec::with_capacity(PROFILES.len());
    for profile in profiles() {
        let enabled = enabled_by_name
            .get(&normalize(profile.name))
            .copied()
            .unwrap_or(false);
        configs.push(profile.config(enabled));
    }

    configs
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, ' ' | '_' | '-'))
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_cover_the_fixed_catalog() {
        let profiles = profiles();
        assert_eq!(profiles.len(), 5);
        assert_eq!(
            profiles
                .iter()
                .map(|profile| profile.name)
                .collect::<Vec<_>>(),
            vec!["Exa", "Tavily", "Firecrawl", "GNOME Desktop", "Obsidian"]
        );
        assert!(profiles.iter().all(|profile| profile.capabilities.tools));
        assert_eq!(profiles[0].args, &["-y", "exa-mcp-server@3.2.1"]);
        assert_eq!(profiles[1].args, &["-y", "tavily-mcp@0.2.20"]);
        assert_eq!(profiles[2].args, &["-y", "firecrawl-mcp@3.22.1"]);
        assert_eq!(profiles[3].args, &["gnome-desktop-mcp==0.1.0"]);
        assert_eq!(
            profiles[4].args,
            &[
                "--from",
                "git+https://github.com/Vasallo94/obsidian-mcp-server.git@ac9889f93c39c3a6a38515fdeb374e61f87c70a2",
                "obsidian-mcp-server"
            ]
        );
    }

    #[test]
    fn lookup_finds_profiles_by_name_and_skill() {
        assert_eq!(
            lookup_profile("exa").map(|profile| profile.name),
            Some("Exa")
        );
        assert_eq!(
            lookup_profile("gnome").map(|profile| profile.name),
            Some("GNOME Desktop")
        );
    }

    #[test]
    fn merged_configs_keeps_enabled_state_and_drops_custom_rows() {
        let existing = vec![
            McpServerConfig {
                name: "Tavily".to_string(),
                enabled: true,
                ..McpServerConfig::default()
            },
            McpServerConfig {
                name: "Custom MCP".to_string(),
                transport: "stdio".to_string(),
                command: Some("custom".to_string()),
                args: Some(vec!["--flag".to_string()]),
                enabled: true,
                ..McpServerConfig::default()
            },
            McpServerConfig {
                name: "Exa".to_string(),
                enabled: true,
                ..McpServerConfig::default()
            },
        ];

        let merged = merged_configs(&existing);
        assert_eq!(
            merged
                .iter()
                .map(|row| row.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Exa", "Tavily", "Firecrawl", "GNOME Desktop", "Obsidian"]
        );
        assert!(merged[0].enabled);
        assert!(merged[1].enabled);
        assert!(!merged[2].enabled);
        assert!(!merged[3].enabled);
        assert!(!merged[4].enabled);
    }
}
