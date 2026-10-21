use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchBackend {
    #[default]
    Local,
    Exa,
    Tavily,
    Firecrawl,
}

impl SearchBackend {
    pub const SERVICES: [Self; 3] = [Self::Exa, Self::Tavily, Self::Firecrawl];

    pub const fn id(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Exa => "exa",
            Self::Tavily => "tavily",
            Self::Firecrawl => "firecrawl",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Local => "Local",
            Self::Exa => "Exa",
            Self::Tavily => "Tavily",
            Self::Firecrawl => "Firecrawl",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WebSearchConfig {
    pub enabled: bool,
    pub backend: SearchBackend,
    pub max_results: usize,
    pub local_endpoint: Option<String>,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: SearchBackend::Local,
            max_results: 5,
            local_endpoint: None,
        }
    }
}

impl WebSearchConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=10).contains(&self.max_results) {
            return Err("Web search result count must be between 1 and 10.");
        }
        if let Some(endpoint) = &self.local_endpoint {
            let url = reqwest::Url::parse(endpoint)
                .map_err(|_| "Enter an absolute HTTP(S) SearXNG search URL.")?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || endpoint.chars().any(char::is_control)
            {
                return Err(
                    "SearXNG URL must use HTTP(S), without credentials, query or fragment.",
                );
            }
        }
        Ok(())
    }
}
