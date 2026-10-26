pub(crate) struct Builtin {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) source: &'static str,
}

pub(crate) const BUILTINS: &[Builtin] = &[
    Builtin {
        name: "command",
        description: "Run one explicitly requested, whitelisted executable with direct arguments.",
        source: include_str!("../../skills/command/SKILL.md"),
    },
    Builtin {
        name: "ping-user",
        description: "Send an explicitly requested Pushover notification.",
        source: include_str!("../../skills/ping-user/SKILL.md"),
    },
    Builtin {
        name: "websearch",
        description: "Search the web using the configured source and cite results.",
        source: include_str!("../../skills/websearch/SKILL.md"),
    },
    Builtin {
        name: "research",
        description: "Research a subject and compile a sourced report.",
        source: include_str!("../../skills/research/SKILL.md"),
    },
    Builtin {
        name: "exa",
        description: "Search the web through Exa MCP and cite returned sources.",
        source: include_str!("../../skills/exa/SKILL.md"),
    },
    Builtin {
        name: "tavily",
        description: "Search the web through Tavily MCP and cite returned sources.",
        source: include_str!("../../skills/tavily/SKILL.md"),
    },
    Builtin {
        name: "firecrawl",
        description: "Search the web through Firecrawl MCP and cite returned sources.",
        source: include_str!("../../skills/firecrawl/SKILL.md"),
    },
    Builtin {
        name: "gnome",
        description: "Operate the GNOME desktop through available live MCP tools.",
        source: include_str!("../../skills/gnome/SKILL.md"),
    },
    Builtin {
        name: "vault",
        description: "Read and manage the internal TCUI Vault.",
        source: include_str!("../../skills/vault/SKILL.md"),
    },
    Builtin {
        name: "caveman",
        description: "Use terse, compressed prose while preserving technical substance.",
        source: include_str!("../../skills/caveman/SKILL.md"),
    },
    Builtin {
        name: "save",
        description: "Create a persistent Markdown Chat file for the TCUI sidebar.",
        source: include_str!("../../skills/save/SKILL.md"),
    },
    Builtin {
        name: "schedule",
        description: "Schedule a local reminder through the host system timer.",
        source: include_str!("../../skills/schedule/SKILL.md"),
    },
    Builtin {
        name: "remindme",
        description: "Schedule a local reminder through the host system timer.",
        source: include_str!("../../skills/remindme/SKILL.md"),
    },
    #[cfg(feature = "memory")]
    Builtin {
        name: "remember",
        description: "Save or recall useful facts from the request and conversation context.",
        source: include_str!("../../skills/remember/SKILL.md"),
    },
    #[cfg(feature = "memory")]
    Builtin {
        name: "memory",
        description: "Save or recall contextual facts, preferences, and machine configuration.",
        source: include_str!("../../skills/memory/SKILL.md"),
    },
    #[cfg(feature = "memory")]
    Builtin {
        name: "memorize",
        description: "Alias for the built-in contextual memory skill.",
        source: include_str!("../../skills/memory/SKILL.md"),
    },
    #[cfg(feature = "memory")]
    Builtin {
        name: "recall",
        description: "Recall local memory without saving or changing it.",
        source: include_str!("../../skills/recall/SKILL.md"),
    },
];

pub(crate) fn source(name: &str) -> Option<&'static str> {
    BUILTINS
        .iter()
        .find(|builtin| builtin.name == name)
        .map(|builtin| builtin.source)
}
