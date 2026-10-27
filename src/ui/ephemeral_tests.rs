use super::{ephemeral::EphemeralPane, UI};
use crate::{app::message::Message, tui::shell::PanelMode};
use ratatui::{
    backend::TestBackend,
    layout::{Position, Rect},
    Terminal,
};

pub(crate) fn install_pane(ui: &mut UI) {
    let main_tab_index = ui.active_tab;
    ui.add_tab(ui.tabs[main_tab_index].tab.clone());
    let tab_index = ui.active_tab;
    ui.ephemeral = Some(EphemeralPane {
        tab_index,
        main_tab_index,
        conversation_id: -1,
        origin_conversation_id: ui.tabs[main_tab_index].active_conversation,
        copied_message_count: 0,
        title: "Ephemeral fork of Original conversation".into(),
        input_id: super::app_tabs::AppTabId::next(),
        previous_right_mode: PanelMode::Closed,
        area: None,
        close_area: None,
    });
    ui.tabs[main_tab_index].input_content = "persistent draft".into();
    ui.tabs[tab_index].input_content = "temporary draft".into();
    ui.tabs[tab_index].active_conversation = -1;
    ui.tabs[main_tab_index].active_conversation = 1;
    ui.tabs[main_tab_index].messages.push(Message::new(
        1,
        "assistant".into(),
        "Persistent history".into(),
    ));
    ui.tabs[tab_index].messages.push(Message::new(
        -1,
        "assistant".into(),
        "Temporary history".into(),
    ));
}

fn surface_text(terminal: &Terminal<TestBackend>, area: Rect) -> String {
    let buffer = terminal.backend().buffer();
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn renders_independent_histories_composers_and_reserved_wide_pane() {
    let mut ui = UI::new();
    install_pane(&mut ui);
    ui.panel_state.right = PanelMode::Closed;
    ui.panel_state.right_wide_width = 60;
    let mut terminal = Terminal::new(TestBackend::new(160, 32)).unwrap();
    terminal.draw(|frame| ui.render(frame)).unwrap();
    let main = ui.chat_area.unwrap();
    let pane = ui.ephemeral.as_ref().unwrap();
    let right = pane.area.unwrap();
    assert_eq!(right.width, 60);
    assert!(main.intersection(right).is_empty());
    let left_text = surface_text(&terminal, main);
    let right_text = surface_text(&terminal, right);
    assert!(left_text.contains("Persistent history"));
    assert!(left_text.contains("persistent draft"));
    assert!(!left_text.contains("Temporary history"));
    assert!(right_text.contains("Temporary history"));
    assert!(right_text.contains("temporary draft"));
    assert!(right_text.contains("Ephemeral fork of Original conversation"));
    assert!(right_text.contains("[X]"));
    assert!(right.contains(Position::new(
        pane.close_area.unwrap().x,
        pane.close_area.unwrap().y
    )));
    assert!(ui.tabs[0].provider_hit_area.is_none());
    assert!(ui.tabs[1].provider_hit_area.is_some());
    for tab in &ui.tabs {
        assert!(tab.input_area.is_some());
        let input = tab.input_area.unwrap();
        assert!(
            main.contains(Position::new(input.x, input.y))
                || right.contains(Position::new(input.x, input.y))
        );
    }
    assert!(ui.tabs[0]
        .input_area
        .unwrap()
        .intersection(ui.tabs[1].input_area.unwrap())
        .is_empty());
}

#[test]
fn narrow_layout_keeps_both_panes_and_wraps_the_full_title() {
    let mut ui = UI::new();
    install_pane(&mut ui);
    ui.sidebar_open = true;
    ui.ephemeral.as_mut().unwrap().title = "Ephemeral fork of Original conversation".into();
    let mut terminal = Terminal::new(TestBackend::new(64, 24)).unwrap();
    terminal.draw(|frame| ui.render(frame)).unwrap();
    let main = ui.chat_area.unwrap();
    let pane = ui.ephemeral.as_ref().unwrap();
    let right = pane.area.unwrap();
    assert!(main.width >= 24);
    assert!(right.width >= 24);
    assert!(main.intersection(right).is_empty());
    let text = surface_text(&terminal, right);
    let compact: String = text
        .chars()
        .filter(|ch| !ch.is_whitespace() && !"│[X]".contains(*ch))
        .collect();
    let title: String = pane
        .title
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect();
    assert!(compact.contains(&title), "{text}");
    assert!(ui.tabs[0].input_area.is_some());
    assert!(ui.tabs[1].input_area.is_some());
}

#[test]
fn shrinking_below_minimum_clears_all_chat_hit_targets() {
    let mut ui = UI::new();
    install_pane(&mut ui);
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    terminal.draw(|frame| ui.render(frame)).unwrap();
    terminal.backend_mut().resize(40, 10);
    terminal.resize(Rect::new(0, 0, 40, 10)).unwrap();
    terminal.draw(|frame| ui.render(frame)).unwrap();
    assert!(ui.chat_area.is_none());
    assert!(ui.ephemeral.as_ref().unwrap().area.is_none());
    assert!(ui.ephemeral.as_ref().unwrap().close_area.is_none());
    for tab in &ui.tabs {
        assert!(tab.input_area.is_none());
        assert!(tab.provider_hit_area.is_none());
        assert!(tab.copy_hit_areas.is_empty());
        assert!(tab.command_hit_areas.is_empty());
        assert!(tab.link_hit_areas.is_empty());
        assert!(tab.dropdown_item_areas.is_empty());
    }
}

#[test]
fn conversation_sidebar_remains_bound_to_main_while_temp_is_focused() {
    let mut ui = UI::new();
    install_pane(&mut ui);
    ui.sidebar_open = true;
    ui.tabs[0].conversations.push(super::ConversationEntry {
        id: 42,
        title: "Main entry".into(),
        created_at: "today".into(),
        updated_at_ms: 1,
        pinned: false,
    });
    ui.tabs[1].conversations.push(super::ConversationEntry {
        id: -1,
        title: "Never list this temp".into(),
        created_at: "today".into(),
        updated_at_ms: 1,
        pinned: false,
    });
    let mut terminal = Terminal::new(TestBackend::new(160, 32)).unwrap();
    terminal.draw(|frame| ui.render(frame)).unwrap();
    let text = terminal.backend().to_string();
    assert!(text.contains("Main entry"));
    assert!(!text.contains("Never list this temp"));
    assert!(ui.mouse_hit_areas.iter().any(|(_, action)| matches!(
        action,
        super::MouseAction::Sidebar(super::sidebar::SidebarAction::LoadConversation(42))
    )));
}

#[test]
fn switching_focus_preserves_drafts_and_closes_outgoing_dropdown() {
    let mut ui = UI::new();
    install_pane(&mut ui);
    ui.tabs[1].model_dropdown_open = true;
    ui.tabs[1].dropdown_item_areas.push(Rect::new(1, 1, 5, 1));
    assert!(ui.focus_chat_pane(0));
    assert_eq!(ui.tabs[0].input_content, "persistent draft");
    assert_eq!(ui.tabs[1].input_content, "temporary draft");
    assert!(!ui.tabs[1].model_dropdown_open);
    assert!(ui.tabs[1].dropdown_item_areas.is_empty());
    assert!(!ui.focus_chat_pane(3));
}

#[test]
fn markdown_copy_command_and_link_controls_stay_inside_their_own_panes() {
    let mut ui = UI::new();
    install_pane(&mut ui);
    ui.markdown_mode = crate::config::app_config::MarkdownMode::Full;
    for (index, tab) in ui.tabs.iter_mut().enumerate() {
        tab.messages[0].content = format!(
            "[Pane link](https://example.com/{index})\n\n```bash tcui-run\necho pane{index}\n```"
        );
    }
    let mut terminal = Terminal::new(TestBackend::new(160, 36)).unwrap();
    terminal.draw(|frame| ui.render(frame)).unwrap();
    let areas = [
        ui.chat_area.unwrap(),
        ui.ephemeral.as_ref().unwrap().area.unwrap(),
    ];
    for (index, tab) in ui.tabs.iter().enumerate() {
        let area = areas[index];
        assert_eq!(
            [
                tab.copy_hit_areas.len(),
                tab.command_hit_areas.len(),
                tab.link_hit_areas.len()
            ],
            [1, 2, 1],
        );
        for (_, hit) in &tab.copy_hit_areas {
            assert_eq!(hit.intersection(area), *hit);
        }
        for (_, _, _, hit) in &tab.command_hit_areas {
            assert_eq!(hit.intersection(area), *hit);
        }
        for (hit, url) in &tab.link_hit_areas {
            assert_eq!(hit.intersection(area), *hit);
            assert_eq!(url, &format!("https://example.com/{index}"));
        }
    }
}
