use std::sync::{OnceLock, RwLock};

use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub is_system: bool,
    pub background: Color,
    pub foreground: Color,
    pub muted: Color,
    pub accent: Color,
    pub accent_alt: Color,
    pub border: Color,
    pub panel: Color,
    pub sidebar: Color,
    pub error: Color,
    pub warning: Color,
    pub success: Color,
    pub info: Color,
    pub code_bg: Color,
    pub code_fg: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub user_bubble: Color,
    pub assistant_bubble: Color,
    pub ansi: [Color; 16],
}

impl ThemeSpec {
    pub fn selected_style(self) -> Style {
        if self.is_system {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default()
                .fg(self.selection_fg)
                .bg(self.selection_bg)
                .add_modifier(Modifier::BOLD)
        }
    }

    pub fn panel_style(self) -> Style {
        Style::default().bg(self.panel).fg(self.foreground)
    }
}

const fn ansi(base: [Color; 8], bright: [Color; 8]) -> [Color; 16] {
    [
        base[0], base[1], base[2], base[3], base[4], base[5], base[6], base[7], bright[0],
        bright[1], bright[2], bright[3], bright[4], bright[5], bright[6], bright[7],
    ]
}

const SYSTEM_ANSI: [Color; 16] = ansi(
    [
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::Gray,
    ],
    [
        Color::DarkGray,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::White,
    ],
);

const SYSTEM: ThemeSpec = ThemeSpec {
    key: "system",
    label: "System",
    is_system: true,
    background: Color::Reset,
    foreground: Color::Reset,
    muted: Color::DarkGray,
    accent: Color::Cyan,
    accent_alt: Color::Blue,
    border: Color::Reset,
    panel: Color::Reset,
    sidebar: Color::Reset,
    error: Color::Red,
    warning: Color::Yellow,
    success: Color::Green,
    info: Color::Cyan,
    code_bg: Color::Reset,
    code_fg: Color::Reset,
    selection_bg: Color::Reset,
    selection_fg: Color::Reset,
    user_bubble: Color::Reset,
    assistant_bubble: Color::Reset,
    ansi: SYSTEM_ANSI,
};

const THEMES: [ThemeSpec; 14] = [
    SYSTEM,
    ThemeSpec {
        key: "gruvbox",
        label: "Gruvbox",
        is_system: false,
        background: Color::Rgb(40, 40, 40),
        foreground: Color::Rgb(235, 219, 178),
        muted: Color::Rgb(146, 131, 116),
        accent: Color::Rgb(131, 165, 152),
        accent_alt: Color::Rgb(215, 153, 33),
        border: Color::Rgb(102, 92, 84),
        panel: Color::Rgb(50, 48, 47),
        sidebar: Color::Rgb(60, 56, 54),
        error: Color::Rgb(251, 73, 52),
        warning: Color::Rgb(250, 189, 47),
        success: Color::Rgb(184, 187, 38),
        info: Color::Rgb(131, 165, 152),
        code_bg: Color::Rgb(29, 32, 33),
        code_fg: Color::Rgb(235, 219, 178),
        selection_bg: Color::Rgb(131, 165, 152),
        selection_fg: Color::Rgb(29, 32, 33),
        user_bubble: Color::Rgb(69, 133, 136),
        assistant_bubble: Color::Rgb(80, 73, 69),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "nord",
        label: "Nord",
        is_system: false,
        background: Color::Rgb(46, 52, 64),
        foreground: Color::Rgb(216, 222, 233),
        muted: Color::Rgb(129, 161, 193),
        accent: Color::Rgb(136, 192, 208),
        accent_alt: Color::Rgb(94, 129, 172),
        border: Color::Rgb(76, 86, 106),
        panel: Color::Rgb(59, 66, 82),
        sidebar: Color::Rgb(67, 76, 94),
        error: Color::Rgb(191, 97, 106),
        warning: Color::Rgb(235, 203, 139),
        success: Color::Rgb(163, 190, 140),
        info: Color::Rgb(136, 192, 208),
        code_bg: Color::Rgb(43, 48, 59),
        code_fg: Color::Rgb(236, 239, 244),
        selection_bg: Color::Rgb(136, 192, 208),
        selection_fg: Color::Rgb(46, 52, 64),
        user_bubble: Color::Rgb(94, 129, 172),
        assistant_bubble: Color::Rgb(67, 76, 94),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "dracula",
        label: "Dracula",
        is_system: false,
        background: Color::Rgb(40, 42, 54),
        foreground: Color::Rgb(248, 248, 242),
        muted: Color::Rgb(98, 114, 164),
        accent: Color::Rgb(189, 147, 249),
        accent_alt: Color::Rgb(139, 233, 253),
        border: Color::Rgb(68, 71, 90),
        panel: Color::Rgb(50, 52, 66),
        sidebar: Color::Rgb(56, 59, 77),
        error: Color::Rgb(255, 85, 85),
        warning: Color::Rgb(241, 250, 140),
        success: Color::Rgb(80, 250, 123),
        info: Color::Rgb(139, 233, 253),
        code_bg: Color::Rgb(33, 34, 44),
        code_fg: Color::Rgb(248, 248, 242),
        selection_bg: Color::Rgb(189, 147, 249),
        selection_fg: Color::Rgb(40, 42, 54),
        user_bubble: Color::Rgb(68, 71, 90),
        assistant_bubble: Color::Rgb(56, 59, 77),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "github",
        label: "GitHub",
        is_system: false,
        background: Color::Rgb(13, 17, 23),
        foreground: Color::Rgb(230, 237, 243),
        muted: Color::Rgb(139, 148, 158),
        accent: Color::Rgb(47, 129, 247),
        accent_alt: Color::Rgb(121, 192, 255),
        border: Color::Rgb(48, 54, 61),
        panel: Color::Rgb(22, 27, 34),
        sidebar: Color::Rgb(18, 22, 29),
        error: Color::Rgb(248, 81, 73),
        warning: Color::Rgb(210, 153, 34),
        success: Color::Rgb(63, 185, 80),
        info: Color::Rgb(121, 192, 255),
        code_bg: Color::Rgb(22, 27, 34),
        code_fg: Color::Rgb(230, 237, 243),
        selection_bg: Color::Rgb(47, 129, 247),
        selection_fg: Color::Rgb(13, 17, 23),
        user_bubble: Color::Rgb(33, 38, 45),
        assistant_bubble: Color::Rgb(22, 27, 34),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "kanagawa",
        label: "Kanagawa",
        is_system: false,
        background: Color::Rgb(31, 31, 40),
        foreground: Color::Rgb(220, 215, 186),
        muted: Color::Rgb(114, 117, 143),
        accent: Color::Rgb(125, 207, 255),
        accent_alt: Color::Rgb(152, 187, 108),
        border: Color::Rgb(84, 84, 109),
        panel: Color::Rgb(42, 42, 55),
        sidebar: Color::Rgb(36, 36, 48),
        error: Color::Rgb(196, 85, 85),
        warning: Color::Rgb(255, 160, 102),
        success: Color::Rgb(135, 183, 101),
        info: Color::Rgb(125, 207, 255),
        code_bg: Color::Rgb(26, 26, 34),
        code_fg: Color::Rgb(220, 215, 186),
        selection_bg: Color::Rgb(125, 207, 255),
        selection_fg: Color::Rgb(31, 31, 40),
        user_bubble: Color::Rgb(54, 54, 73),
        assistant_bubble: Color::Rgb(42, 42, 55),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "catppuccin",
        label: "Catppuccin",
        is_system: false,
        background: Color::Rgb(30, 30, 46),
        foreground: Color::Rgb(205, 214, 244),
        muted: Color::Rgb(166, 173, 200),
        accent: Color::Rgb(137, 180, 250),
        accent_alt: Color::Rgb(203, 166, 247),
        border: Color::Rgb(88, 91, 112),
        panel: Color::Rgb(49, 50, 68),
        sidebar: Color::Rgb(24, 24, 37),
        error: Color::Rgb(243, 139, 168),
        warning: Color::Rgb(250, 179, 135),
        success: Color::Rgb(166, 227, 161),
        info: Color::Rgb(137, 180, 250),
        code_bg: Color::Rgb(24, 24, 37),
        code_fg: Color::Rgb(205, 214, 244),
        selection_bg: Color::Rgb(137, 180, 250),
        selection_fg: Color::Rgb(24, 24, 37),
        user_bubble: Color::Rgb(69, 71, 90),
        assistant_bubble: Color::Rgb(49, 50, 68),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "material",
        label: "Material",
        is_system: false,
        background: Color::Rgb(38, 50, 56),
        foreground: Color::Rgb(238, 255, 255),
        muted: Color::Rgb(176, 190, 197),
        accent: Color::Rgb(130, 170, 255),
        accent_alt: Color::Rgb(199, 146, 234),
        border: Color::Rgb(55, 71, 79),
        panel: Color::Rgb(45, 58, 64),
        sidebar: Color::Rgb(32, 42, 48),
        error: Color::Rgb(255, 83, 112),
        warning: Color::Rgb(255, 203, 107),
        success: Color::Rgb(195, 232, 141),
        info: Color::Rgb(130, 170, 255),
        code_bg: Color::Rgb(31, 41, 46),
        code_fg: Color::Rgb(238, 255, 255),
        selection_bg: Color::Rgb(130, 170, 255),
        selection_fg: Color::Rgb(31, 41, 46),
        user_bubble: Color::Rgb(55, 71, 79),
        assistant_bubble: Color::Rgb(45, 58, 64),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "matrix",
        label: "Matrix",
        is_system: false,
        background: Color::Rgb(5, 14, 8),
        foreground: Color::Rgb(144, 238, 144),
        muted: Color::Rgb(76, 128, 76),
        accent: Color::Rgb(0, 255, 102),
        accent_alt: Color::Rgb(102, 255, 153),
        border: Color::Rgb(24, 64, 32),
        panel: Color::Rgb(10, 20, 12),
        sidebar: Color::Rgb(8, 16, 10),
        error: Color::Rgb(255, 85, 85),
        warning: Color::Rgb(204, 255, 102),
        success: Color::Rgb(0, 255, 102),
        info: Color::Rgb(102, 255, 153),
        code_bg: Color::Rgb(0, 12, 0),
        code_fg: Color::Rgb(144, 238, 144),
        selection_bg: Color::Rgb(0, 255, 102),
        selection_fg: Color::Rgb(5, 14, 8),
        user_bubble: Color::Rgb(14, 34, 18),
        assistant_bubble: Color::Rgb(10, 20, 12),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "monokai",
        label: "Monokai",
        is_system: false,
        background: Color::Rgb(39, 40, 34),
        foreground: Color::Rgb(248, 248, 242),
        muted: Color::Rgb(117, 113, 94),
        accent: Color::Rgb(166, 226, 46),
        accent_alt: Color::Rgb(102, 217, 239),
        border: Color::Rgb(73, 72, 62),
        panel: Color::Rgb(49, 50, 42),
        sidebar: Color::Rgb(43, 44, 36),
        error: Color::Rgb(249, 38, 114),
        warning: Color::Rgb(253, 151, 31),
        success: Color::Rgb(166, 226, 46),
        info: Color::Rgb(102, 217, 239),
        code_bg: Color::Rgb(30, 31, 28),
        code_fg: Color::Rgb(248, 248, 242),
        selection_bg: Color::Rgb(102, 217, 239),
        selection_fg: Color::Rgb(30, 31, 28),
        user_bubble: Color::Rgb(73, 72, 62),
        assistant_bubble: Color::Rgb(49, 50, 42),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "zenburn",
        label: "Zenburn",
        is_system: false,
        background: Color::Rgb(63, 63, 63),
        foreground: Color::Rgb(220, 220, 204),
        muted: Color::Rgb(112, 128, 105),
        accent: Color::Rgb(143, 191, 173),
        accent_alt: Color::Rgb(240, 223, 175),
        border: Color::Rgb(94, 94, 94),
        panel: Color::Rgb(58, 58, 58),
        sidebar: Color::Rgb(54, 54, 54),
        error: Color::Rgb(204, 147, 147),
        warning: Color::Rgb(240, 223, 175),
        success: Color::Rgb(127, 159, 127),
        info: Color::Rgb(143, 191, 173),
        code_bg: Color::Rgb(53, 53, 53),
        code_fg: Color::Rgb(220, 220, 204),
        selection_bg: Color::Rgb(143, 191, 173),
        selection_fg: Color::Rgb(53, 53, 53),
        user_bubble: Color::Rgb(94, 94, 94),
        assistant_bubble: Color::Rgb(58, 58, 58),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "solarized",
        label: "Solarized",
        is_system: false,
        background: Color::Rgb(0, 43, 54),
        foreground: Color::Rgb(131, 148, 150),
        muted: Color::Rgb(88, 110, 117),
        accent: Color::Rgb(38, 139, 210),
        accent_alt: Color::Rgb(42, 161, 152),
        border: Color::Rgb(7, 54, 66),
        panel: Color::Rgb(0, 51, 63),
        sidebar: Color::Rgb(0, 46, 57),
        error: Color::Rgb(220, 50, 47),
        warning: Color::Rgb(181, 137, 0),
        success: Color::Rgb(133, 153, 0),
        info: Color::Rgb(38, 139, 210),
        code_bg: Color::Rgb(7, 54, 66),
        code_fg: Color::Rgb(147, 161, 161),
        selection_bg: Color::Rgb(38, 139, 210),
        selection_fg: Color::Rgb(0, 43, 54),
        user_bubble: Color::Rgb(7, 54, 66),
        assistant_bubble: Color::Rgb(0, 51, 63),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "tokyo-night",
        label: "Tokyo Night",
        is_system: false,
        background: Color::Rgb(26, 27, 38),
        foreground: Color::Rgb(192, 202, 245),
        muted: Color::Rgb(86, 95, 137),
        accent: Color::Rgb(122, 162, 247),
        accent_alt: Color::Rgb(187, 154, 247),
        border: Color::Rgb(65, 72, 104),
        panel: Color::Rgb(36, 40, 59),
        sidebar: Color::Rgb(31, 35, 53),
        error: Color::Rgb(247, 118, 142),
        warning: Color::Rgb(224, 175, 104),
        success: Color::Rgb(158, 206, 106),
        info: Color::Rgb(122, 162, 247),
        code_bg: Color::Rgb(22, 22, 30),
        code_fg: Color::Rgb(192, 202, 245),
        selection_bg: Color::Rgb(122, 162, 247),
        selection_fg: Color::Rgb(22, 22, 30),
        user_bubble: Color::Rgb(49, 53, 82),
        assistant_bubble: Color::Rgb(36, 40, 59),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "opencode",
        label: "OpenCode",
        is_system: false,
        background: Color::Rgb(14, 18, 24),
        foreground: Color::Rgb(229, 231, 235),
        muted: Color::Rgb(148, 163, 184),
        accent: Color::Rgb(0, 184, 148),
        accent_alt: Color::Rgb(59, 130, 246),
        border: Color::Rgb(39, 49, 66),
        panel: Color::Rgb(20, 25, 34),
        sidebar: Color::Rgb(17, 22, 30),
        error: Color::Rgb(239, 68, 68),
        warning: Color::Rgb(245, 158, 11),
        success: Color::Rgb(16, 185, 129),
        info: Color::Rgb(59, 130, 246),
        code_bg: Color::Rgb(10, 14, 20),
        code_fg: Color::Rgb(229, 231, 235),
        selection_bg: Color::Rgb(0, 184, 148),
        selection_fg: Color::Rgb(10, 14, 20),
        user_bubble: Color::Rgb(30, 41, 59),
        assistant_bubble: Color::Rgb(20, 25, 34),
        ansi: SYSTEM_ANSI,
    },
];

static ACTIVE_THEME: OnceLock<RwLock<ThemeSpec>> = OnceLock::new();

fn active_slot() -> &'static RwLock<ThemeSpec> {
    ACTIVE_THEME.get_or_init(|| RwLock::new(SYSTEM))
}

pub fn active_theme() -> ThemeSpec {
    *active_slot().read().expect("theme lock poisoned")
}

pub fn set_active_theme(name: &str) -> ThemeSpec {
    let theme = resolve_theme(name);
    *active_slot().write().expect("theme lock poisoned") = theme;
    theme
}

pub fn resolve_theme(name: &str) -> ThemeSpec {
    find_theme(name).unwrap_or(SYSTEM)
}

pub fn find_theme(name: &str) -> Option<ThemeSpec> {
    let normalized = normalize_key(name);
    THEMES.iter().copied().find(|theme| {
        normalize_key(theme.key) == normalized || normalize_key(theme.label) == normalized
    })
}

pub fn theme_keys() -> Vec<&'static str> {
    THEMES.iter().map(|theme| theme.key).collect()
}

pub fn theme_labels() -> Vec<&'static str> {
    THEMES.iter().map(|theme| theme.label).collect()
}

pub fn theme_label(name: &str) -> &'static str {
    resolve_theme(name).label
}

pub fn canonical_theme_key(name: &str) -> &'static str {
    resolve_theme(name).key
}

fn normalize_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
