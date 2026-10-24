use crate::config::McpServerConfig;
use serde::Serialize;

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
    pub capabilities: McpCapabilities,
}

impl McpProfile {
    pub fn config(self, enabled: bool) -> McpServerConfig {
        let endpoint = self.hosted_endpoint();
        McpServerConfig {
            name: self.name.to_string(),
            transport: if endpoint.is_some() {
                "streamable_http"
            } else {
                "stdio"
            }
            .to_string(),
            command: endpoint.is_none().then(|| self.program.to_string()),
            args: endpoint
                .is_none()
                .then(|| self.args.iter().map(|arg| (*arg).to_string()).collect()),
            url: endpoint.map(ToString::to_string),
            enabled,
        }
    }

    pub fn hosted_endpoint(&self) -> Option<&'static str> {
        match self.skill {
            "exa" => Some("https://mcp.exa.ai/mcp"),
            "tavily" => Some("https://mcp.tavily.com/mcp/"),
            "firecrawl" => Some("https://mcp.firecrawl.dev/v2/mcp"),
            _ => None,
        }
    }
}

const PROFILES: [McpProfile; 4] = [
    McpProfile {
        name: "Exa",
        skill: "exa",
        description: "Exa Search",
        program: "npx",
        args: &["-y", "exa-mcp-server@3.2.1"],
        api_key_env: Some("EXA_API_KEY"),
        key_store_name: Some("Exa Search"),
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
        capabilities: McpCapabilities::tools_only(),
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
    let mut configs = existing.to_vec();
    for profile in profiles() {
        if !existing
            .iter()
            .any(|row| lookup_profile(&row.name) == Some(profile))
        {
            configs.push(profile.config(false));
        }
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
    fn merged_configs_preserves_explicit_transport_and_custom_rows() {
        let existing = vec![
            McpServerConfig {
                name: "Tavily".to_string(),
                transport: "stdio".to_string(),
                command: Some("my-tavily".to_string()),
                args: Some(vec!["--flag".to_string()]),
                enabled: true,
                ..McpServerConfig::default()
            },
            McpServerConfig {
                name: "Custom MCP".to_string(),
                url: Some("https://example.com/mcp".to_string()),
                enabled: true,
                ..McpServerConfig::default()
            },
        ];
        let merged = merged_configs(&existing);
        assert_eq!(&merged[..existing.len()], existing.as_slice());
        assert_eq!(merged.len(), 5);
        assert!(merged[2..].iter().all(|row| !row.enabled));
        assert!(merged[2..4]
            .iter()
            .all(|row| row.transport == "streamable_http"));
    }
}
