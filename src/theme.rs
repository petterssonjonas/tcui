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
    pub card_bg: Color,
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

    pub fn sidebar_style(self) -> Style {
        Style::default().bg(self.sidebar).fg(self.foreground)
    }

    pub fn card_style(self) -> Style {
        Style::default().bg(self.card_bg).fg(self.foreground)
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
    background: Color::Rgb(20, 20, 24),
    foreground: Color::Reset,
    muted: Color::DarkGray,
    accent: Color::Cyan,
    accent_alt: Color::Blue,
    border: Color::Reset,
    panel: Color::Rgb(24, 24, 28),
    sidebar: Color::Rgb(20, 20, 24),
    card_bg: Color::Rgb(20, 20, 24),
    error: Color::Red,
    warning: Color::Yellow,
    success: Color::Green,
    info: Color::Cyan,
    code_bg: Color::Rgb(20, 20, 24),
    code_fg: Color::White,
    selection_bg: Color::DarkGray,
    selection_fg: Color::White,
    user_bubble: Color::Rgb(40, 40, 46),
    assistant_bubble: Color::Rgb(32, 32, 36),
    ansi: SYSTEM_ANSI,
};

const THEMES: [ThemeSpec; 15] = [
    SYSTEM,
    ThemeSpec {
        key: "gruvbox-dark-low-contrast",
        label: "Gruvbox Dark Low Contrast",
        is_system: false,
        background: Color::Rgb(29, 32, 33),
        foreground: Color::Rgb(235, 219, 178),
        muted: Color::Rgb(168, 153, 132),
        accent: Color::Rgb(131, 165, 152),
        accent_alt: Color::Rgb(184, 187, 38),
        border: Color::Rgb(60, 56, 54),
        panel: Color::Rgb(40, 40, 40),
        sidebar: Color::Rgb(40, 40, 40),
        card_bg: Color::Rgb(60, 56, 54),
        error: Color::Rgb(251, 73, 52),
        warning: Color::Rgb(250, 189, 47),
        success: Color::Rgb(184, 187, 38),
        info: Color::Rgb(131, 165, 152),
        code_bg: Color::Rgb(29, 32, 33),
        code_fg: Color::Rgb(131, 165, 152),
        selection_bg: Color::Rgb(80, 73, 69),
        selection_fg: Color::Rgb(131, 165, 152),
        user_bubble: Color::Rgb(50, 48, 47),
        assistant_bubble: Color::Rgb(40, 40, 40),
        ansi: ansi(
            [
                Color::Rgb(29, 32, 33),
                Color::Rgb(204, 36, 29),
                Color::Rgb(152, 151, 26),
                Color::Rgb(215, 153, 33),
                Color::Rgb(69, 133, 136),
                Color::Rgb(177, 98, 134),
                Color::Rgb(104, 157, 106),
                Color::Rgb(168, 153, 132),
            ],
            [
                Color::Rgb(168, 153, 132),
                Color::Rgb(251, 73, 52),
                Color::Rgb(184, 187, 38),
                Color::Rgb(250, 189, 47),
                Color::Rgb(131, 165, 152),
                Color::Rgb(211, 134, 155),
                Color::Rgb(142, 192, 124),
                Color::Rgb(235, 219, 178),
            ],
        ),
    },
    ThemeSpec {
        key: "gruvbox-dark-high-contrast",
        label: "Gruvbox Dark High Contrast",
        is_system: false,
        background: Color::Rgb(29, 32, 33),
        foreground: Color::Rgb(235, 219, 178),
        muted: Color::Rgb(168, 153, 132),
        accent: Color::Rgb(69, 133, 136),
        accent_alt: Color::Rgb(243, 128, 25),
        border: Color::Rgb(80, 73, 69),
        panel: Color::Rgb(40, 40, 40),
        sidebar: Color::Rgb(40, 40, 40),
        card_bg: Color::Rgb(60, 56, 54),
        error: Color::Rgb(204, 36, 29),
        warning: Color::Rgb(250, 189, 47),
        success: Color::Rgb(152, 151, 26),
        info: Color::Rgb(69, 133, 136),
        code_bg: Color::Rgb(29, 32, 33),
        code_fg: Color::Rgb(69, 133, 136),
        selection_bg: Color::Rgb(80, 73, 69),
        selection_fg: Color::Rgb(69, 133, 136),
        user_bubble: Color::Rgb(50, 48, 47),
        assistant_bubble: Color::Rgb(40, 40, 40),
        ansi: ansi(
            [
                Color::Rgb(29, 32, 33),
                Color::Rgb(204, 36, 29),
                Color::Rgb(152, 151, 26),
                Color::Rgb(215, 153, 33),
                Color::Rgb(69, 133, 136),
                Color::Rgb(177, 98, 134),
                Color::Rgb(104, 157, 106),
                Color::Rgb(168, 153, 132),
            ],
            [
                Color::Rgb(146, 131, 116),
                Color::Rgb(251, 73, 52),
                Color::Rgb(184, 187, 38),
                Color::Rgb(250, 189, 47),
                Color::Rgb(131, 165, 152),
                Color::Rgb(211, 134, 155),
                Color::Rgb(142, 192, 124),
                Color::Rgb(235, 219, 178),
            ],
        ),
    },
    ThemeSpec {
        key: "nord",
        label: "Nord",
        is_system: false,
        background: Color::Rgb(43, 48, 59),
        foreground: Color::Rgb(216, 222, 233),
        muted: Color::Rgb(129, 161, 193),
        accent: Color::Rgb(136, 192, 208),
        accent_alt: Color::Rgb(94, 129, 172),
        border: Color::Rgb(76, 86, 106),
        panel: Color::Rgb(59, 66, 82),
        sidebar: Color::Rgb(43, 48, 59),
        card_bg: Color::Rgb(52, 59, 73),
        error: Color::Rgb(191, 97, 106),
        warning: Color::Rgb(235, 203, 139),
        success: Color::Rgb(163, 190, 140),
        info: Color::Rgb(136, 192, 208),
        code_bg: Color::Rgb(43, 48, 59),
        code_fg: Color::Rgb(236, 239, 244),
        selection_bg: Color::Rgb(76, 86, 106),
        selection_fg: Color::Rgb(216, 222, 233),
        user_bubble: Color::Rgb(52, 59, 73),
        assistant_bubble: Color::Rgb(46, 52, 64),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "dracula",
        label: "Dracula",
        is_system: false,
        background: Color::Rgb(33, 34, 44),
        foreground: Color::Rgb(248, 248, 242),
        muted: Color::Rgb(98, 114, 164),
        accent: Color::Rgb(189, 147, 249),
        accent_alt: Color::Rgb(139, 233, 253),
        border: Color::Rgb(68, 71, 90),
        panel: Color::Rgb(50, 52, 66),
        sidebar: Color::Rgb(33, 34, 44),
        card_bg: Color::Rgb(42, 44, 57),
        error: Color::Rgb(255, 85, 85),
        warning: Color::Rgb(241, 250, 140),
        success: Color::Rgb(80, 250, 123),
        info: Color::Rgb(139, 233, 253),
        code_bg: Color::Rgb(33, 34, 44),
        code_fg: Color::Rgb(248, 248, 242),
        selection_bg: Color::Rgb(68, 71, 90),
        selection_fg: Color::Rgb(248, 248, 242),
        user_bubble: Color::Rgb(42, 44, 57),
        assistant_bubble: Color::Rgb(35, 37, 49),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "github",
        label: "GitHub",
        is_system: false,
        background: Color::Rgb(22, 27, 34),
        foreground: Color::Rgb(230, 237, 243),
        muted: Color::Rgb(139, 148, 158),
        accent: Color::Rgb(47, 129, 247),
        accent_alt: Color::Rgb(121, 192, 255),
        border: Color::Rgb(48, 54, 61),
        panel: Color::Rgb(22, 27, 34),
        sidebar: Color::Rgb(22, 27, 34),
        card_bg: Color::Rgb(20, 24, 31),
        error: Color::Rgb(248, 81, 73),
        warning: Color::Rgb(210, 153, 34),
        success: Color::Rgb(63, 185, 80),
        info: Color::Rgb(121, 192, 255),
        code_bg: Color::Rgb(22, 27, 34),
        code_fg: Color::Rgb(230, 237, 243),
        selection_bg: Color::Rgb(48, 54, 61),
        selection_fg: Color::Rgb(230, 237, 243),
        user_bubble: Color::Rgb(20, 24, 31),
        assistant_bubble: Color::Rgb(18, 22, 29),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "kanagawa",
        label: "Kanagawa",
        is_system: false,
        background: Color::Rgb(26, 26, 34),
        foreground: Color::Rgb(220, 215, 186),
        muted: Color::Rgb(114, 117, 143),
        accent: Color::Rgb(125, 207, 255),
        accent_alt: Color::Rgb(152, 187, 108),
        border: Color::Rgb(84, 84, 109),
        panel: Color::Rgb(42, 42, 55),
        sidebar: Color::Rgb(26, 26, 34),
        card_bg: Color::Rgb(39, 39, 51),
        error: Color::Rgb(196, 85, 85),
        warning: Color::Rgb(255, 160, 102),
        success: Color::Rgb(135, 183, 101),
        info: Color::Rgb(125, 207, 255),
        code_bg: Color::Rgb(26, 26, 34),
        code_fg: Color::Rgb(220, 215, 186),
        selection_bg: Color::Rgb(84, 84, 109),
        selection_fg: Color::Rgb(220, 215, 186),
        user_bubble: Color::Rgb(39, 39, 51),
        assistant_bubble: Color::Rgb(36, 36, 48),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "catppuccin",
        label: "Catppuccin",
        is_system: false,
        background: Color::Rgb(24, 24, 37),
        foreground: Color::Rgb(205, 214, 244),
        muted: Color::Rgb(166, 173, 200),
        accent: Color::Rgb(137, 180, 250),
        accent_alt: Color::Rgb(203, 166, 247),
        border: Color::Rgb(88, 91, 112),
        panel: Color::Rgb(49, 50, 68),
        sidebar: Color::Rgb(24, 24, 37),
        card_bg: Color::Rgb(36, 37, 52),
        error: Color::Rgb(243, 139, 168),
        warning: Color::Rgb(250, 179, 135),
        success: Color::Rgb(166, 227, 161),
        info: Color::Rgb(137, 180, 250),
        code_bg: Color::Rgb(24, 24, 37),
        code_fg: Color::Rgb(205, 214, 244),
        selection_bg: Color::Rgb(88, 91, 112),
        selection_fg: Color::Rgb(205, 214, 244),
        user_bubble: Color::Rgb(36, 37, 52),
        assistant_bubble: Color::Rgb(24, 24, 37),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "material",
        label: "Material",
        is_system: false,
        background: Color::Rgb(31, 41, 46),
        foreground: Color::Rgb(238, 255, 255),
        muted: Color::Rgb(176, 190, 197),
        accent: Color::Rgb(130, 170, 255),
        accent_alt: Color::Rgb(199, 146, 234),
        border: Color::Rgb(55, 71, 79),
        panel: Color::Rgb(45, 58, 64),
        sidebar: Color::Rgb(31, 41, 46),
        card_bg: Color::Rgb(38, 50, 56),
        error: Color::Rgb(255, 83, 112),
        warning: Color::Rgb(255, 203, 107),
        success: Color::Rgb(195, 232, 141),
        info: Color::Rgb(130, 170, 255),
        code_bg: Color::Rgb(31, 41, 46),
        code_fg: Color::Rgb(238, 255, 255),
        selection_bg: Color::Rgb(55, 71, 79),
        selection_fg: Color::Rgb(238, 255, 255),
        user_bubble: Color::Rgb(38, 50, 56),
        assistant_bubble: Color::Rgb(32, 42, 48),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "matrix",
        label: "Matrix",
        is_system: false,
        background: Color::Rgb(0, 12, 0),
        foreground: Color::Rgb(144, 238, 144),
        muted: Color::Rgb(76, 128, 76),
        accent: Color::Rgb(0, 255, 102),
        accent_alt: Color::Rgb(102, 255, 153),
        border: Color::Rgb(24, 64, 32),
        panel: Color::Rgb(10, 20, 12),
        sidebar: Color::Rgb(0, 12, 0),
        card_bg: Color::Rgb(9, 18, 11),
        error: Color::Rgb(255, 85, 85),
        warning: Color::Rgb(204, 255, 102),
        success: Color::Rgb(0, 255, 102),
        info: Color::Rgb(102, 255, 153),
        code_bg: Color::Rgb(0, 12, 0),
        code_fg: Color::Rgb(144, 238, 144),
        selection_bg: Color::Rgb(24, 64, 32),
        selection_fg: Color::Rgb(144, 238, 144),
        user_bubble: Color::Rgb(9, 18, 11),
        assistant_bubble: Color::Rgb(8, 16, 10),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "monokai",
        label: "Monokai",
        is_system: false,
        background: Color::Rgb(30, 31, 28),
        foreground: Color::Rgb(248, 248, 242),
        muted: Color::Rgb(117, 113, 94),
        accent: Color::Rgb(166, 226, 46),
        accent_alt: Color::Rgb(102, 217, 239),
        border: Color::Rgb(73, 72, 62),
        panel: Color::Rgb(49, 50, 42),
        sidebar: Color::Rgb(30, 31, 28),
        card_bg: Color::Rgb(46, 47, 39),
        error: Color::Rgb(249, 38, 114),
        warning: Color::Rgb(253, 151, 31),
        success: Color::Rgb(166, 226, 46),
        info: Color::Rgb(102, 217, 239),
        code_bg: Color::Rgb(30, 31, 28),
        code_fg: Color::Rgb(248, 248, 242),
        selection_bg: Color::Rgb(73, 72, 62),
        selection_fg: Color::Rgb(248, 248, 242),
        user_bubble: Color::Rgb(46, 47, 39),
        assistant_bubble: Color::Rgb(43, 44, 36),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "zenburn",
        label: "Zenburn",
        is_system: false,
        background: Color::Rgb(53, 53, 53),
        foreground: Color::Rgb(220, 220, 204),
        muted: Color::Rgb(112, 128, 105),
        accent: Color::Rgb(143, 191, 173),
        accent_alt: Color::Rgb(240, 223, 175),
        border: Color::Rgb(94, 94, 94),
        panel: Color::Rgb(58, 58, 58),
        sidebar: Color::Rgb(53, 53, 53),
        card_bg: Color::Rgb(56, 56, 56),
        error: Color::Rgb(204, 147, 147),
        warning: Color::Rgb(240, 223, 175),
        success: Color::Rgb(127, 159, 127),
        info: Color::Rgb(143, 191, 173),
        code_bg: Color::Rgb(53, 53, 53),
        code_fg: Color::Rgb(220, 220, 204),
        selection_bg: Color::Rgb(94, 94, 94),
        selection_fg: Color::Rgb(220, 220, 204),
        user_bubble: Color::Rgb(56, 56, 56),
        assistant_bubble: Color::Rgb(54, 54, 54),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "solarized",
        label: "Solarized",
        is_system: false,
        background: Color::Rgb(7, 54, 66),
        foreground: Color::Rgb(131, 148, 150),
        muted: Color::Rgb(88, 110, 117),
        accent: Color::Rgb(38, 139, 210),
        accent_alt: Color::Rgb(42, 161, 152),
        border: Color::Rgb(7, 54, 66),
        panel: Color::Rgb(0, 51, 63),
        sidebar: Color::Rgb(7, 54, 66),
        card_bg: Color::Rgb(0, 48, 60),
        error: Color::Rgb(220, 50, 47),
        warning: Color::Rgb(181, 137, 0),
        success: Color::Rgb(133, 153, 0),
        info: Color::Rgb(38, 139, 210),
        code_bg: Color::Rgb(7, 54, 66),
        code_fg: Color::Rgb(147, 161, 161),
        selection_bg: Color::Rgb(7, 54, 66),
        selection_fg: Color::Rgb(131, 148, 150),
        user_bubble: Color::Rgb(0, 48, 60),
        assistant_bubble: Color::Rgb(0, 46, 57),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "tokyo-night",
        label: "Tokyo Night",
        is_system: false,
        background: Color::Rgb(22, 22, 30),
        foreground: Color::Rgb(192, 202, 245),
        muted: Color::Rgb(86, 95, 137),
        accent: Color::Rgb(122, 162, 247),
        accent_alt: Color::Rgb(187, 154, 247),
        border: Color::Rgb(65, 72, 104),
        panel: Color::Rgb(36, 40, 59),
        sidebar: Color::Rgb(22, 22, 30),
        card_bg: Color::Rgb(33, 37, 56),
        error: Color::Rgb(247, 118, 142),
        warning: Color::Rgb(224, 175, 104),
        success: Color::Rgb(158, 206, 106),
        info: Color::Rgb(122, 162, 247),
        code_bg: Color::Rgb(22, 22, 30),
        code_fg: Color::Rgb(192, 202, 245),
        selection_bg: Color::Rgb(65, 72, 104),
        selection_fg: Color::Rgb(192, 202, 245),
        user_bubble: Color::Rgb(33, 37, 56),
        assistant_bubble: Color::Rgb(31, 35, 53),
        ansi: SYSTEM_ANSI,
    },
    ThemeSpec {
        key: "opencode",
        label: "OpenCode",
        is_system: false,
        background: Color::Rgb(10, 14, 20),
        foreground: Color::Rgb(229, 231, 235),
        muted: Color::Rgb(148, 163, 184),
        accent: Color::Rgb(0, 184, 148),
        accent_alt: Color::Rgb(59, 130, 246),
        border: Color::Rgb(39, 49, 66),
        panel: Color::Rgb(20, 25, 34),
        sidebar: Color::Rgb(10, 14, 20),
        card_bg: Color::Rgb(18, 23, 32),
        error: Color::Rgb(239, 68, 68),
        warning: Color::Rgb(245, 158, 11),
        success: Color::Rgb(16, 185, 129),
        info: Color::Rgb(59, 130, 246),
        code_bg: Color::Rgb(10, 14, 20),
        code_fg: Color::Rgb(229, 231, 235),
        selection_bg: Color::Rgb(39, 49, 66),
        selection_fg: Color::Rgb(229, 231, 235),
        user_bubble: Color::Rgb(18, 23, 32),
        assistant_bubble: Color::Rgb(17, 22, 30),
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
    let normalized = if normalized == "gruvbox" {
        "gruvboxdarklowcontrast"
    } else {
        normalized.as_str()
    };
    THEMES.iter().copied().find(|theme| {
        normalize_key(theme.key) == normalized || normalize_key(theme.label) == normalized
    })
}

pub fn theme_keys() -> Vec<&'static str> {
    THEMES.iter().map(|theme| theme.key).collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gruvbox_variants_match_exported_surface_and_accent_colors() {
        let low = find_theme("gruvbox-dark-low-contrast").expect("low contrast theme");
        let high = find_theme("gruvbox-dark-high-contrast").expect("high contrast theme");

        assert_eq!(find_theme("gruvbox"), Some(low));
        assert_eq!(low.background, Color::Rgb(29, 32, 33));
        assert_eq!(low.panel, Color::Rgb(40, 40, 40));
        assert_eq!(low.sidebar, Color::Rgb(40, 40, 40));
        assert_eq!(low.user_bubble, Color::Rgb(50, 48, 47));
        assert_eq!(low.assistant_bubble, Color::Rgb(40, 40, 40));
        assert_eq!(low.accent, Color::Rgb(131, 165, 152));
        assert_eq!(low.ansi[0], Color::Rgb(29, 32, 33));
        assert_eq!(low.ansi[8], Color::Rgb(168, 153, 132));
        assert_eq!(low.ansi[15], Color::Rgb(235, 219, 178));

        assert_eq!(high.background, Color::Rgb(29, 32, 33));
        assert_eq!(high.panel, Color::Rgb(40, 40, 40));
        assert_eq!(high.sidebar, Color::Rgb(40, 40, 40));
        assert_eq!(high.user_bubble, Color::Rgb(50, 48, 47));
        assert_eq!(high.assistant_bubble, Color::Rgb(40, 40, 40));
        assert_eq!(high.accent, Color::Rgb(69, 133, 136));
        assert_eq!(high.ansi[0], Color::Rgb(29, 32, 33));
        assert_eq!(high.ansi[8], Color::Rgb(146, 131, 116));
        assert_eq!(high.ansi[15], Color::Rgb(235, 219, 178));
        for index in (0..16).filter(|index| *index != 8) {
            assert_eq!(high.ansi[index], low.ansi[index]);
        }
    }
}
