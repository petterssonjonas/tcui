use super::*;

impl SettingsPopup {
    pub(super) fn render_general(&mut self, f: &mut Frame, area: Rect) {
        self.general_hit_areas = GeneralHitAreas::default();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .margin(1)
            .split(area);

        let theme_focused =
            self.active_tab == SettingsTab::General && self.general_focus == GeneralFocus::Theme;
        let theme_widget = Paragraph::new(format!("{} ▼", crate::theme::theme_label(&self.theme)))
            .block(
                Block::default()
                    .title(" Theme ")
                    .borders(Borders::ALL)
                    .border_style(if theme_focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            )
            .style(if theme_focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            });
        self.general_hit_areas.theme = Some(chunks[0]);
        f.render_widget(theme_widget, chunks[0]);

        let user_align_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::UserAlignment;
        let user_align = Paragraph::new(format!("{} ▼", self.user_alignment))
            .block(
                Block::default()
                    .title(" User alignment ")
                    .borders(Borders::ALL)
                    .border_style(if user_align_focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            )
            .style(if user_align_focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            });
        self.general_hit_areas.user_alignment = Some(chunks[1]);
        f.render_widget(user_align, chunks[1]);

        let ai_align_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::AiAlignment;
        let ai_align = Paragraph::new(format!("{} ▼", self.ai_alignment))
            .block(
                Block::default()
                    .title(" AI alignment ")
                    .borders(Borders::ALL)
                    .border_style(if ai_align_focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            )
            .style(if ai_align_focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            });
        self.general_hit_areas.ai_alignment = Some(chunks[2]);
        f.render_widget(ai_align, chunks[2]);

        let artifact_save_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::ArtifactSaveDir;
        let artifact_save_widget = Paragraph::new(self.artifact_save_dir.as_str())
            .block(
                Block::default()
                    .title(" Artifact save dir ")
                    .borders(Borders::ALL)
                    .border_style(if artifact_save_focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            )
            .style(if artifact_save_focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            });
        self.general_hit_areas.artifact_save_dir = Some(chunks[3]);
        f.render_widget(artifact_save_widget, chunks[3]);

        let selector_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::ShowSelector;
        let selector_lines = vec![Line::from(vec![
            Span::raw(if self.show_selector { "[✓] " } else { "[ ] " }),
            Span::styled(
                "Show provider/model selector",
                if selector_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ])];
        let selector_widget = Paragraph::new(selector_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(if selector_focused {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::DarkGray)
                }),
        );
        self.general_hit_areas.show_selector = Some(chunks[4]);
        f.render_widget(selector_widget, chunks[4]);

        let collapse_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::ShowChatScrollbar;
        let scrollbar_lines = vec![Line::from(vec![
            Span::raw(if self.show_chat_scrollbar {
                "[✓] "
            } else {
                "[ ] "
            }),
            Span::styled(
                "Show chat scrollbar",
                if collapse_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ])];
        let scrollbar_widget = Paragraph::new(scrollbar_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(if collapse_focused {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::DarkGray)
                }),
        );
        self.general_hit_areas.show_chat_scrollbar = Some(chunks[5]);
        f.render_widget(scrollbar_widget, chunks[5]);

        let collapse_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::CollapseThinking;
        let collapse_lines = vec![Line::from(vec![
            Span::raw(if self.collapse_thinking {
                "[✓] "
            } else {
                "[ ] "
            }),
            Span::styled(
                "Fold thinking by default",
                if collapse_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ])];
        let collapse_widget = Paragraph::new(collapse_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(if collapse_focused {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::DarkGray)
                }),
        );
        self.general_hit_areas.collapse_thinking = Some(chunks[6]);
        f.render_widget(collapse_widget, chunks[6]);

        let kitty_toggle_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::KittyEnhancedText;
        let kitty_toggle = Paragraph::new(vec![Line::from(vec![
            Span::raw(if self.kitty_enhanced_text {
                "[✓] "
            } else {
                "[ ] "
            }),
            Span::styled(
                "Use kitty enhanced text",
                if kitty_toggle_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ])])
        .block(Block::default().borders(Borders::ALL).border_style(
            if kitty_toggle_focused {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ));
        self.general_hit_areas.kitty_enhanced_text = Some(chunks[7]);
        f.render_widget(kitty_toggle, chunks[7]);

        let kitty_scale_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::KittyTextScale;
        let kitty_scale = Paragraph::new(format!("{} ▼", self.kitty_heading_downscale.label()))
            .block(
                Block::default()
                    .title(" Chat heading size ")
                    .borders(Borders::ALL)
                    .border_style(if kitty_scale_focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            )
            .style(if kitty_scale_focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            });
        self.general_hit_areas.kitty_text_scale = Some(chunks[8]);
        f.render_widget(kitty_scale, chunks[8]);

        let web_toggle_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::WebSearchEnabled;
        let web_toggle = Paragraph::new(vec![Line::from(vec![
            Span::raw(if self.web_search_enabled {
                "[✓] "
            } else {
                "[ ] "
            }),
            Span::styled(
                "Enable web search",
                if web_toggle_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ])])
        .block(Block::default().borders(Borders::ALL).border_style(
            if web_toggle_focused {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ));
        self.general_hit_areas.web_search_enabled = Some(chunks[9]);
        f.render_widget(web_toggle, chunks[9]);

        let quit_focused = self.active_tab == SettingsTab::General
            && self.general_focus == GeneralFocus::QuitConfirmation;
        let quit_toggle =
            Paragraph::new(vec![Line::from(vec![
                Span::raw(if self.quit_confirmation {
                    "[✓] "
                } else {
                    "[ ] "
                }),
                Span::styled(
                    "Confirm before quit",
                    if quit_focused {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
            ])])
            .block(Block::default().borders(Borders::ALL).border_style(
                if quit_focused {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ));
        self.general_hit_areas.quit_confirmation = Some(chunks[10]);
        f.render_widget(quit_toggle, chunks[10]);

        if let Some(dropdown) = self.general_dropdown_open {
            match dropdown {
                GeneralDropdown::Theme => {
                    let items = crate::theme::theme_labels();
                    let list_items: Vec<ListItem> = items
                        .iter()
                        .map(|label| {
                            let style = if *label == crate::theme::theme_label(&self.theme) {
                                Style::default().fg(Color::Black).bg(Color::Cyan)
                            } else {
                                Style::default().fg(Color::White)
                            };
                            ListItem::new(*label).style(style)
                        })
                        .collect();
                    let dropdown_area =
                        Self::dropdown_area_below(chunks[0], items.len() as u16 + 2);
                    let list = List::new(list_items).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan))
                            .style(Style::default().bg(Color::Black)),
                    );
                    f.render_widget(Clear, dropdown_area);
                    f.render_widget(list, dropdown_area);
                    self.general_hit_areas.dropdown_items = (0..items.len())
                        .map(|i| Rect {
                            x: dropdown_area.x + 1,
                            y: dropdown_area.y + 1 + i as u16,
                            width: dropdown_area.width - 2,
                            height: 1,
                        })
                        .collect();
                }
                GeneralDropdown::UserAlignment => {
                    let items = ["left", "middle", "right"];
                    let list_items: Vec<ListItem> = items
                        .iter()
                        .map(|name| {
                            let style = if *name == self.user_alignment.to_string().as_str() {
                                Style::default().fg(Color::Black).bg(Color::Cyan)
                            } else {
                                Style::default().fg(Color::White)
                            };
                            ListItem::new(*name).style(style)
                        })
                        .collect();
                    let dropdown_area = Self::dropdown_area_below(chunks[1], 5);
                    let list = List::new(list_items).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan))
                            .style(Style::default().bg(Color::Black)),
                    );
                    f.render_widget(Clear, dropdown_area);
                    f.render_widget(list, dropdown_area);

                    let item_height = 1;
                    let mut item_areas = Vec::new();
                    for i in 0..items.len() {
                        item_areas.push(Rect {
                            x: dropdown_area.x + 1,
                            y: dropdown_area.y + 1 + (i as u16 * item_height),
                            width: dropdown_area.width - 2,
                            height: item_height,
                        });
                    }
                    self.general_hit_areas.dropdown_items = item_areas;
                }
                GeneralDropdown::AiAlignment => {
                    let items = ["left", "middle", "right"];
                    let list_items: Vec<ListItem> = items
                        .iter()
                        .map(|name| {
                            let style = if *name == self.ai_alignment.to_string().as_str() {
                                Style::default().fg(Color::Black).bg(Color::Cyan)
                            } else {
                                Style::default().fg(Color::White)
                            };
                            ListItem::new(*name).style(style)
                        })
                        .collect();
                    let dropdown_area = Self::dropdown_area_below(chunks[2], 5);
                    let list = List::new(list_items).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan))
                            .style(Style::default().bg(Color::Black)),
                    );
                    f.render_widget(Clear, dropdown_area);
                    f.render_widget(list, dropdown_area);

                    let item_height = 1;
                    let mut item_areas = Vec::new();
                    for i in 0..items.len() {
                        item_areas.push(Rect {
                            x: dropdown_area.x + 1,
                            y: dropdown_area.y + 1 + (i as u16 * item_height),
                            width: dropdown_area.width - 2,
                            height: item_height,
                        });
                    }
                    self.general_hit_areas.dropdown_items = item_areas;
                }
                GeneralDropdown::KittyTextScale => {
                    let dropdown_area = Self::dropdown_area_below(chunks[8], 5);
                    let list_items: Vec<ListItem> = KITTY_HEADING_SIZE_OPTIONS
                        .iter()
                        .map(|downscale| {
                            let style = if *downscale == self.kitty_heading_downscale {
                                Style::default().fg(Color::Black).bg(Color::Cyan)
                            } else {
                                Style::default().fg(Color::White)
                            };
                            ListItem::new(downscale.label()).style(style)
                        })
                        .collect();
                    let list = List::new(list_items).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan))
                            .style(Style::default().bg(Color::Black)),
                    );
                    f.render_widget(Clear, dropdown_area);
                    f.render_widget(list, dropdown_area);

                    self.general_hit_areas.dropdown_items = (0..KITTY_HEADING_SIZE_OPTIONS.len())
                        .map(|i| Rect {
                            x: dropdown_area.x + 1,
                            y: dropdown_area.y + 1 + i as u16,
                            width: dropdown_area.width - 2,
                            height: 1,
                        })
                        .collect();
                }
            }
        }
    }

    pub fn select_general_dropdown_item(&mut self, idx: usize) -> bool {
        match self.general_dropdown_open {
            Some(GeneralDropdown::Theme) => {
                if let Some(theme) = crate::theme::theme_keys().get(idx) {
                    let changed = crate::theme::canonical_theme_key(&self.theme) != *theme;
                    self.theme = (*theme).to_string();
                    self.general_dropdown_open = None;
                    return changed;
                }
            }
            Some(GeneralDropdown::UserAlignment) => {
                if let Some(alignment) = ALIGNMENT_OPTIONS.get(idx).copied() {
                    self.user_alignment = alignment;
                }
            }
            Some(GeneralDropdown::AiAlignment) => {
                if let Some(alignment) = ALIGNMENT_OPTIONS.get(idx).copied() {
                    self.ai_alignment = alignment;
                }
            }
            Some(GeneralDropdown::KittyTextScale) => {
                if let Some(downscale) = KITTY_HEADING_SIZE_OPTIONS.get(idx).copied() {
                    self.kitty_heading_downscale = downscale;
                }
            }
            None => {}
        }
        self.general_dropdown_open = None;
        false
    }

    pub fn general_dropdown_len(&self) -> usize {
        match self.general_dropdown_open {
            Some(GeneralDropdown::Theme) => crate::theme::theme_keys().len(),
            Some(GeneralDropdown::UserAlignment) => ALIGNMENT_OPTIONS.len(),
            Some(GeneralDropdown::AiAlignment) => ALIGNMENT_OPTIONS.len(),
            Some(GeneralDropdown::KittyTextScale) => KITTY_HEADING_SIZE_OPTIONS.len(),
            None => 0,
        }
    }

    pub fn general_dropdown_current_idx(&self) -> usize {
        match self.general_dropdown_open {
            Some(GeneralDropdown::Theme) => crate::theme::theme_keys()
                .iter()
                .position(|theme| *theme == crate::theme::canonical_theme_key(&self.theme))
                .unwrap_or(0),
            Some(GeneralDropdown::UserAlignment) => ALIGNMENT_OPTIONS
                .iter()
                .position(|a| *a == self.user_alignment)
                .unwrap_or(0),
            Some(GeneralDropdown::AiAlignment) => ALIGNMENT_OPTIONS
                .iter()
                .position(|a| *a == self.ai_alignment)
                .unwrap_or(0),
            Some(GeneralDropdown::KittyTextScale) => KITTY_HEADING_SIZE_OPTIONS
                .iter()
                .position(|downscale| *downscale == self.kitty_heading_downscale)
                .unwrap_or(0),
            None => 0,
        }
    }

    pub fn general_dropdown_up(&mut self) {
        let len = self.general_dropdown_len();
        if len == 0 {
            return;
        }
        let idx = self.general_dropdown_current_idx();
        let new_idx = if idx == 0 { len - 1 } else { idx - 1 };
        self.select_general_dropdown_item(new_idx);
        if let Some(dropdown) = self.focus_to_dropdown() {
            self.general_dropdown_open = Some(dropdown);
        }
    }

    pub fn general_dropdown_down(&mut self) {
        let len = self.general_dropdown_len();
        if len == 0 {
            return;
        }
        let idx = self.general_dropdown_current_idx();
        let new_idx = (idx + 1) % len;
        self.select_general_dropdown_item(new_idx);
        if let Some(dropdown) = self.focus_to_dropdown() {
            self.general_dropdown_open = Some(dropdown);
        }
    }

    pub(super) fn focus_to_dropdown(&self) -> Option<GeneralDropdown> {
        match self.general_focus {
            GeneralFocus::Theme => Some(GeneralDropdown::Theme),
            GeneralFocus::UserAlignment => Some(GeneralDropdown::UserAlignment),
            GeneralFocus::AiAlignment => Some(GeneralDropdown::AiAlignment),
            GeneralFocus::ArtifactSaveDir => None,
            GeneralFocus::ShowSelector => None,
            GeneralFocus::ShowChatScrollbar => None,
            GeneralFocus::CollapseThinking => None,
            GeneralFocus::KittyEnhancedText => None,
            GeneralFocus::KittyTextScale => Some(GeneralDropdown::KittyTextScale),
            GeneralFocus::WebSearchEnabled => None,
            GeneralFocus::QuitConfirmation => None,
        }
    }

    pub fn toggle_general_dropdown(&mut self, dropdown: GeneralDropdown) {
        if self.general_dropdown_open == Some(dropdown) {
            self.general_dropdown_open = None;
        } else {
            self.general_dropdown_open = Some(dropdown);
        }
    }

    pub fn close_general_dropdown(&mut self) {
        self.general_dropdown_open = None;
    }
}
