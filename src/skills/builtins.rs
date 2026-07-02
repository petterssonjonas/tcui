pub(crate) struct Builtin {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) source: &'static str,
}

pub(crate) const BUILTINS: &[Builtin] = &[
    Builtin {
        name: "websearch",
        description: "Run a focused local web search and integrate sourced results.",
        source: include_str!("../../skills/websearch/SKILL.md"),
    },
    Builtin {
        name: "research",
        description: "Research a subject and compile a sourced report.",
        source: include_str!("../../skills/research/SKILL.md"),
    },
    Builtin {
        name: "exa",
        description: "Research the web with the live Exa MCP tools.",
        source: include_str!("../../skills/exa/SKILL.md"),
    },
    Builtin {
        name: "tavily",
        description: "Research the web with the live Tavily MCP tools.",
        source: include_str!("../../skills/tavily/SKILL.md"),
    },
    Builtin {
        name: "firecrawl",
        description: "Search or extract web content with the live Firecrawl MCP tools.",
        source: include_str!("../../skills/firecrawl/SKILL.md"),
    },
    Builtin {
        name: "gnome",
        description: "Operate the GNOME desktop through available live MCP tools.",
        source: include_str!("../../skills/gnome/SKILL.md"),
    },
    Builtin {
        name: "obsidian",
        description: "Search, read, or explicitly update Obsidian notes through live MCP tools.",
        source: include_str!("../../skills/obsidian/SKILL.md"),
    },
    Builtin {
        name: "caveman",
        description: "Use terse, compressed prose while preserving technical substance.",
        source: include_str!("../../skills/caveman/SKILL.md"),
    },
    Builtin {
        name: "save",
        description: "Create a Markdown artifact for the TermChatUI sidebar.",
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
        description: "Save one durable fact or preference to local memory.",
        source: include_str!("../../skills/remember/SKILL.md"),
    },
    #[cfg(feature = "memory")]
    Builtin {
        name: "memory",
        description: "Search, read, write, forget, or inspect local memory.",
        source: include_str!("../../skills/memory/SKILL.md"),
    },
    #[cfg(feature = "memory")]
    Builtin {
        name: "memorize",
        description: "Search, read, write, forget, or inspect local memory.",
        source: include_str!("../../skills/memory/SKILL.md"),
    },
];

pub(crate) fn source(name: &str) -> Option<&'static str> {
    BUILTINS
        .iter()
        .find(|builtin| builtin.name == name)
        .map(|builtin| builtin.source)
}
