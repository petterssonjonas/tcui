use super::*;
use crate::ui::modals::list_popup::{ListPopup, ListPopupAction, ListPopupItem};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};

fn enter() -> KeyEvent {
    KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
}

fn render(app: &mut TuiApp) {
    let mut terminal = Terminal::new(TestBackend::new(160, 32)).unwrap();
    terminal.draw(|frame| app.ui.render(frame)).unwrap();
}

#[test]
fn temp_typed_commands_parse_exact_tokens_without_sending_messages() {
    crate::app::tests::with_test_app("temp-input-parse", |app| {
        for (text, close) in [(" /temp ", false), ("/temp close", true)] {
            app.replace_input_content(text.into());
            let action = app.handle_key(enter());
            assert!(matches!(
                (close, action),
                (false, Some(Action::OpenEphemeral)) | (true, Some(Action::CloseEphemeral))
            ));
            assert!(app.ui.tabs[0].input_content.is_empty());
        }
        for text in ["/temporary", "/tempest", "/temp closer", "hello /temp"] {
            assert!(app.action_for_slash_command(text).is_none());
        }
        assert!(app.ui.tabs[0].messages.is_empty());
    });
}

#[test]
fn temp_keyboard_completions_execute_open_and_close() {
    crate::app::tests::with_test_app("temp-input-completion", |app| {
        for (text, close) in [("/temp", false), ("/temp close", true)] {
            for action in [
                ListPopupAction::InsertText(text.into()),
                ListPopupAction::ReplaceInput(text.into()),
            ] {
                app.ui.list_popup = Some(ListPopup::selectable(
                    "Commands",
                    "Empty",
                    vec![ListPopupItem::action(text, action)],
                ));
                let action = app.handle_key(enter());
                assert!(matches!(
                    (close, action),
                    (false, Some(Action::OpenEphemeral)) | (true, Some(Action::CloseEphemeral))
                ));
                assert!(app.ui.list_popup.is_none());
            }
        }
    });
}

#[test]
fn temp_mouse_completion_executes_instead_of_inserting_a_command() {
    crate::app::tests::with_test_app("temp-input-mouse-completion", |app| {
        app.ui.list_popup = Some(ListPopup::selectable(
            "Commands",
            "Empty",
            vec![ListPopupItem::action(
                "/temp",
                ListPopupAction::ReplaceInput("/temp".into()),
            )],
        ));
        render(app);
        let popup = app
            .ui
            .list_popup
            .as_ref()
            .unwrap()
            .popup_area_in(app.ui.last_area.unwrap());
        assert!(matches!(
            app.handle_mouse_click(popup.x + 1, popup.y + 1),
            Some(Action::OpenEphemeral)
        ));
        assert!(app.ui.tabs[0].input_content.is_empty());
    });
}

#[test]
fn pane_mouse_click_and_wheel_focus_correct_history_without_losing_drafts() {
    crate::app::tests::with_test_app("temp-input-pointer", |app| {
        crate::ui::ephemeral_tests::install_pane(&mut app.ui);
        render(app);
        let main_input = app.ui.tabs[0].input_area.unwrap();
        app.handle_mouse_click(main_input.x + 1, main_input.y + 1);
        assert_eq!(app.ui.active_tab, 0);
        let pane = app.ui.ephemeral.as_ref().unwrap().area.unwrap();
        app.ui.terminals.focused = true;
        app.handle_mouse_scroll(pane.x + 2, pane.y + 5, KeyModifiers::NONE, true);
        assert_eq!(app.ui.active_tab, 1);
        assert!(!app.ui.terminals.focused);
        assert_eq!(app.ui.tabs[0].scroll_offset, 0);
        assert_eq!(app.ui.tabs[1].scroll_offset, 3);
        assert_eq!(app.ui.tabs[0].input_content, "persistent draft");
        assert_eq!(app.ui.tabs[1].input_content, "temporary draft");
        let main = app.ui.chat_area.unwrap();
        app.handle_mouse_scroll(main.x + 2, main.y + 3, KeyModifiers::NONE, true);
        assert_eq!(app.ui.active_tab, 0);
        assert_eq!(app.ui.tabs[0].scroll_offset, 3);
    });
}

#[test]
fn alt_arrow_focus_actions_switch_composers_and_close_control_routes_close() {
    crate::app::tests::with_test_app("temp-input-focus-keys", |app| {
        crate::ui::ephemeral_tests::install_pane(&mut app.ui);
        assert!(matches!(
            app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT)),
            Some(Action::FocusChatPane(0))
        ));
        app.focus_chat_pane(0);
        app.handle_key(KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE));
        assert!(matches!(
            app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT)),
            Some(Action::FocusChatPane(1))
        ));
        app.focus_chat_pane(1);
        assert_eq!(app.ui.tabs[0].input_content, "!persistent draft");
        assert_eq!(app.ui.tabs[1].input_content, "temporary draft");
        render(app);
        let close = app.ui.ephemeral.as_ref().unwrap().close_area.unwrap();
        assert!(matches!(
            app.handle_mouse_click(close.x, close.y),
            Some(Action::CloseEphemeral)
        ));
    });
}

#[test]
fn modals_block_temp_commands_keyboard_focus_close_and_wheel() {
    crate::app::tests::with_test_app("temp-input-modal-priority", |app| {
        crate::ui::ephemeral_tests::install_pane(&mut app.ui);
        app.replace_input_content("/temp close".into());
        render(app);
        let close = app.ui.ephemeral.as_ref().unwrap().close_area.unwrap();
        app.ui.active_modal = Some(crate::ui::Modal::QuitConfirm);
        assert!(app.handle_key(enter()).is_none());
        assert!(app
            .handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT))
            .is_none());
        app.handle_mouse_scroll(2, 5, KeyModifiers::NONE, true);
        assert_eq!(app.ui.active_tab, 1);
        assert_eq!(app.ui.tabs[1].scroll_offset, 0);
        assert!(!matches!(
            app.handle_mouse_click(close.x, close.y),
            Some(Action::CloseEphemeral)
        ));
        assert_eq!(app.ui.tabs[1].input_content, "/temp close");
    });
}

#[test]
fn sidebar_new_chat_focuses_main_before_returning_action() {
    crate::app::tests::with_test_app("temp-input-sidebar-main", |app| {
        crate::ui::ephemeral_tests::install_pane(&mut app.ui);
        app.ui.sidebar_open = true;
        render(app);
        let area = app
            .ui
            .mouse_hit_areas
            .iter()
            .find_map(|(area, action)| {
                matches!(
                    action,
                    crate::ui::MouseAction::Sidebar(crate::ui::sidebar::SidebarAction::NewChat)
                )
                .then_some(*area)
            })
            .unwrap();
        assert!(matches!(
            app.handle_mouse_click(area.x, area.y),
            Some(Action::NewChat)
        ));
        assert_eq!(app.ui.active_tab, 0);
        assert_eq!(app.ui.tabs[1].input_content, "temporary draft");
    });
}

#[test]
fn command_review_uses_the_clicked_pane_message() {
    crate::app::tests::with_test_app("temp-input-command-review", |app| {
        crate::ui::ephemeral_tests::install_pane(&mut app.ui);
        app.ui.markdown_mode = crate::config::app_config::MarkdownMode::Full;
        app.ui.tabs[0].messages[0].content = "```bash tcui-run\necho persistent\n```".into();
        app.ui.tabs[1].messages[0].content = "```bash tcui-run\necho temporary\n```".into();
        render(app);
        let run = app.ui.tabs[0]
            .command_hit_areas
            .iter()
            .find_map(|(_, _, action, area)| {
                (*action == crate::ui::chat_tab::CommandBlockAction::Run).then_some(*area)
            })
            .expect("main Run control");
        app.handle_mouse_click(run.x, run.y);
        assert_eq!(app.ui.active_tab, 0);
        assert!(app
            .ui
            .command_review
            .as_ref()
            .expect("review")
            .command()
            .contains("echo persistent"));
        app.ui.command_review = None;
        render(app);
        let run = app.ui.tabs[1]
            .command_hit_areas
            .iter()
            .find_map(|(_, _, action, area)| {
                (*action == crate::ui::chat_tab::CommandBlockAction::Run).then_some(*area)
            })
            .expect("temp Run control");
        app.handle_mouse_click(run.x, run.y);
        assert_eq!(app.ui.active_tab, 1);
        assert!(app
            .ui
            .command_review
            .as_ref()
            .expect("review")
            .command()
            .contains("echo temporary"));
    });
}
