use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelMode {
    Closed,
    #[default]
    Thin,
    Wide,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToastPosition {
    TopLeft,
    TopCenter,
    #[default]
    TopRight,
    Center,
    Off,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusWidgetPlacement {
    pub id: String,
    pub row: u8,
    pub area: u8,
}

impl Default for StatusWidgetPlacement {
    fn default() -> Self {
        Self {
            id: String::new(),
            row: 1,
            area: 1,
        }
    }
}

impl StatusWidgetPlacement {
    fn resolve(self) -> Self {
        Self {
            row: self.row.clamp(1, 2),
            area: self.area.clamp(1, 6),
            ..self
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TuiConfig {
    pub left_sidebar_mode: PanelMode,
    pub right_sidebar_mode: PanelMode,
    pub left_thin_width: u16,
    pub left_wide_width: u16,
    pub right_thin_width: u16,
    pub right_wide_width: u16,
    pub status_bar_rows: u8,
    pub status_widgets: Vec<StatusWidgetPlacement>,
    pub helper_enabled: bool,
    pub toast_position: ToastPosition,
    pub pinned_commands: Vec<String>,
    pub keybinding_overrides: BTreeMap<String, String>,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            left_sidebar_mode: PanelMode::Thin,
            right_sidebar_mode: PanelMode::Closed,
            left_thin_width: 28,
            left_wide_width: 42,
            right_thin_width: 32,
            right_wide_width: 56,
            status_bar_rows: 1,
            status_widgets: Vec::new(),
            helper_enabled: true,
            toast_position: ToastPosition::TopRight,
            pinned_commands: Vec::new(),
            keybinding_overrides: BTreeMap::new(),
        }
    }
}

impl TuiConfig {
    /// Returns a copy with all numeric fields clamped to valid ranges.
    /// Called automatically at the deserialization boundary via
    /// `deserialize_tui` on the `AppConfig.tui` field.
    pub fn resolve(self) -> Self {
        Self {
            left_thin_width: self.left_thin_width.clamp(16, 48),
            left_wide_width: self.left_wide_width.clamp(32, 64),
            right_thin_width: self.right_thin_width.clamp(20, 56),
            right_wide_width: self.right_wide_width.clamp(40, 80),
            status_bar_rows: self.status_bar_rows.clamp(1, 2),
            status_widgets: self
                .status_widgets
                .into_iter()
                .map(|w| w.resolve())
                .collect(),
            ..self
        }
    }
}

/// Serde helper: deserialize a `TuiConfig` then clamp all values.
/// Used via `#[serde(default, deserialize_with = "deserialize_tui")]` on
/// the `tui` field of `AppConfig`.
pub fn deserialize_tui<'de, D>(deserializer: D) -> Result<TuiConfig, D::Error>
where
    D: Deserializer<'de>,
{
    TuiConfig::deserialize(deserializer).map(|c| c.resolve())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_produces_expected_values() {
        let config = TuiConfig::default();
        assert_eq!(config.left_sidebar_mode, PanelMode::Thin);
        assert_eq!(config.right_sidebar_mode, PanelMode::Closed);
        assert_eq!(config.left_thin_width, 28);
        assert_eq!(config.left_wide_width, 42);
        assert_eq!(config.right_thin_width, 32);
        assert_eq!(config.right_wide_width, 56);
        assert_eq!(config.status_bar_rows, 1);
        assert!(config.status_widgets.is_empty());
        assert!(config.helper_enabled);
        assert_eq!(config.toast_position, ToastPosition::TopRight);
        assert!(config.pinned_commands.is_empty());
        assert!(config.keybinding_overrides.is_empty());
    }

    #[test]
    fn missing_tui_section_uses_defaults() {
        let toml = r#"
theme = "system"
default_model = "gpt-4o"
default_provider = "openai"
"#;
        let config: crate::config::AppConfig = toml::from_str(toml).expect("parse config");
        assert_eq!(config.tui, TuiConfig::default());
    }

    #[test]
    fn keybinding_overrides_parse_from_tui_section() {
        let toml = r#"
[tui.keybinding_overrides]
open_palette = "ctrl+o"
"#;
        let config: crate::config::AppConfig = toml::from_str(toml).expect("parse config");
        assert_eq!(
            config.tui.keybinding_overrides.get("open_palette"),
            Some(&"ctrl+o".to_string())
        );
    }

    #[test]
    fn left_thin_width_above_max_clamps_to_48() {
        let toml = r#"
[tui]
left_thin_width = 999
"#;
        let config: crate::config::AppConfig = toml::from_str(toml).expect("parse config");
        assert_eq!(config.tui.left_thin_width, 48);
    }

    #[test]
    fn left_thin_width_below_min_clamps_to_16() {
        let toml = r#"
[tui]
left_thin_width = 5
"#;
        let config: crate::config::AppConfig = toml::from_str(toml).expect("parse config");
        assert_eq!(config.tui.left_thin_width, 16);
    }

    #[test]
    fn all_widths_clamp_to_their_ranges() {
        let toml = r#"
[tui]
left_thin_width = 1
left_wide_width = 1
right_thin_width = 1
right_wide_width = 1
"#;
        let config: crate::config::AppConfig = toml::from_str(toml).expect("parse config");
        assert_eq!(config.tui.left_thin_width, 16);
        assert_eq!(config.tui.left_wide_width, 32);
        assert_eq!(config.tui.right_thin_width, 20);
        assert_eq!(config.tui.right_wide_width, 40);

        let toml = r#"
[tui]
left_thin_width = 999
left_wide_width = 999
right_thin_width = 999
right_wide_width = 999
"#;
        let config: crate::config::AppConfig = toml::from_str(toml).expect("parse config");
        assert_eq!(config.tui.left_thin_width, 48);
        assert_eq!(config.tui.left_wide_width, 64);
        assert_eq!(config.tui.right_thin_width, 56);
        assert_eq!(config.tui.right_wide_width, 80);
    }

    #[test]
    fn status_bar_rows_clamps_to_1_or_2() {
        let toml = r#"
[tui]
status_bar_rows = 0
"#;
        let config: crate::config::AppConfig = toml::from_str(toml).expect("parse config");
        assert_eq!(config.tui.status_bar_rows, 1);

        let toml = r#"
[tui]
status_bar_rows = 5
"#;
        let config: crate::config::AppConfig = toml::from_str(toml).expect("parse config");
        assert_eq!(config.tui.status_bar_rows, 2);
    }

    #[test]
    fn status_widget_row_above_max_clamps_to_2() {
        let toml = r#"
[[tui.status_widgets]]
id = "model"
row = 3
area = 1
"#;
        let config: crate::config::AppConfig = toml::from_str(toml).expect("parse config");
        assert_eq!(config.tui.status_widgets.len(), 1);
        assert_eq!(config.tui.status_widgets[0].row, 2);
        assert_eq!(config.tui.status_widgets[0].area, 1);
        assert_eq!(config.tui.status_widgets[0].id, "model");
    }

    #[test]
    fn status_widget_area_above_max_clamps_to_6() {
        let toml = r#"
[[tui.status_widgets]]
id = "clock"
row = 1
area = 9
"#;
        let config: crate::config::AppConfig = toml::from_str(toml).expect("parse config");
        assert_eq!(config.tui.status_widgets[0].area, 6);
    }

    #[test]
    fn status_widget_row_and_area_below_min_clamps_to_1() {
        let toml = r#"
[[tui.status_widgets]]
id = "x"
row = 0
area = 0
"#;
        let config: crate::config::AppConfig = toml::from_str(toml).expect("parse config");
        assert_eq!(config.tui.status_widgets[0].row, 1);
        assert_eq!(config.tui.status_widgets[0].area, 1);
    }

    #[test]
    fn toast_position_off_parses_and_round_trips() {
        let toml = r#"
[tui]
toast_position = "off"
"#;
        let config: crate::config::AppConfig = toml::from_str(toml).expect("parse config");
        assert_eq!(config.tui.toast_position, ToastPosition::Off);

        let serialized = toml::to_string_pretty(&config).expect("serialize config");
        let reparsed: crate::config::AppConfig =
            toml::from_str(&serialized).expect("reparse config");
        assert_eq!(reparsed.tui.toast_position, ToastPosition::Off);
    }

    #[test]
    fn toast_position_all_variants_round_trip() {
        for variant in [
            ToastPosition::TopLeft,
            ToastPosition::TopCenter,
            ToastPosition::TopRight,
            ToastPosition::Center,
            ToastPosition::Off,
        ] {
            let config = TuiConfig {
                toast_position: variant,
                ..TuiConfig::default()
            };
            let serialized = toml::to_string_pretty(&config).expect("serialize");
            let reparsed: TuiConfig = toml::from_str(&serialized).expect("reparse");
            assert_eq!(reparsed.toast_position, variant);
        }
    }

    #[test]
    fn typoed_toast_position_rejects_with_error() {
        let toml = r#"
[tui]
toast_position = "bottom"
"#;
        let result: Result<crate::config::AppConfig, _> = toml::from_str(toml);
        assert!(result.is_err(), "typoed toast position should reject");
    }

    #[test]
    fn keybinding_overrides_preserve_btreemap_order_on_round_trip() {
        let toml = r#"
[tui]
[tui.keybinding_overrides]
"quit" = "ctrl+q"
"save" = "ctrl+s"
"open" = "ctrl+o"
"#;
        let config: crate::config::AppConfig = toml::from_str(toml).expect("parse config");

        // BTreeMap iterates in sorted key order
        let keys: Vec<&String> = config.tui.keybinding_overrides.keys().collect();
        assert_eq!(keys, vec!["open", "quit", "save"]);

        let serialized = toml::to_string_pretty(&config).expect("serialize");
        let reparsed: crate::config::AppConfig = toml::from_str(&serialized).expect("reparse");
        let keys_after: Vec<&String> = reparsed.tui.keybinding_overrides.keys().collect();
        assert_eq!(keys_after, vec!["open", "quit", "save"]);
        assert_eq!(
            reparsed
                .tui
                .keybinding_overrides
                .get("quit")
                .map(|s| s.as_str()),
            Some("ctrl+q")
        );
    }

    #[test]
    fn pinned_commands_round_trip() {
        let toml = r#"
[tui]
pinned_commands = ["help", "settings", "quit"]
"#;
        let config: crate::config::AppConfig = toml::from_str(toml).expect("parse config");
        assert_eq!(config.tui.pinned_commands, vec!["help", "settings", "quit"]);

        let serialized = toml::to_string_pretty(&config).expect("serialize");
        let reparsed: crate::config::AppConfig = toml::from_str(&serialized).expect("reparse");
        assert_eq!(
            reparsed.tui.pinned_commands,
            vec!["help", "settings", "quit"]
        );
    }

    #[test]
    fn partial_tui_section_uses_defaults_for_missing_fields() {
        let toml = r#"
[tui]
helper_enabled = false
"#;
        let config: crate::config::AppConfig = toml::from_str(toml).expect("parse config");
        assert!(!config.tui.helper_enabled);
        // All other fields should be defaults
        assert_eq!(config.tui.left_sidebar_mode, PanelMode::Thin);
        assert_eq!(config.tui.left_thin_width, 28);
        assert_eq!(config.tui.toast_position, ToastPosition::TopRight);
        assert!(config.tui.status_widgets.is_empty());
    }

    #[test]
    fn full_tui_section_round_trips() {
        let toml = r#"
[tui]
left_sidebar_mode = "wide"
right_sidebar_mode = "thin"
left_thin_width = 20
left_wide_width = 50
right_thin_width = 25
right_wide_width = 60
status_bar_rows = 2
helper_enabled = false
toast_position = "top_left"
pinned_commands = ["cmd1", "cmd2"]

[[tui.status_widgets]]
id = "model"
row = 1
area = 3

[[tui.status_widgets]]
id = "tokens"
row = 2
area = 6

[tui.keybinding_overrides]
"quit" = "ctrl+q"
"save" = "ctrl+s"
"#;
        let config: crate::config::AppConfig = toml::from_str(toml).expect("parse config");
        assert_eq!(config.tui.left_sidebar_mode, PanelMode::Wide);
        assert_eq!(config.tui.right_sidebar_mode, PanelMode::Thin);
        assert_eq!(config.tui.left_thin_width, 20);
        assert_eq!(config.tui.left_wide_width, 50);
        assert_eq!(config.tui.right_thin_width, 25);
        assert_eq!(config.tui.right_wide_width, 60);
        assert_eq!(config.tui.status_bar_rows, 2);
        assert!(!config.tui.helper_enabled);
        assert_eq!(config.tui.toast_position, ToastPosition::TopLeft);
        assert_eq!(config.tui.pinned_commands, vec!["cmd1", "cmd2"]);
        assert_eq!(config.tui.status_widgets.len(), 2);
        assert_eq!(config.tui.status_widgets[0].id, "model");
        assert_eq!(config.tui.status_widgets[0].row, 1);
        assert_eq!(config.tui.status_widgets[0].area, 3);
        assert_eq!(config.tui.status_widgets[1].id, "tokens");
        assert_eq!(config.tui.status_widgets[1].row, 2);
        assert_eq!(config.tui.status_widgets[1].area, 6);
        assert_eq!(
            config
                .tui
                .keybinding_overrides
                .get("quit")
                .map(|s| s.as_str()),
            Some("ctrl+q")
        );

        let serialized = toml::to_string_pretty(&config).expect("serialize");
        let reparsed: crate::config::AppConfig = toml::from_str(&serialized).expect("reparse");
        assert_eq!(reparsed.tui, config.tui);
    }

    #[test]
    fn negative_width_rejects_with_deserialization_error() {
        let toml = r#"
[tui]
left_thin_width = -5
"#;
        let result: Result<crate::config::AppConfig, _> = toml::from_str(toml);
        assert!(result.is_err(), "negative width should reject, not clamp");
    }
}
