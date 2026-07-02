use super::*;

impl SettingsPopup {
    pub(super) fn render_providers(&mut self, f: &mut Frame, area: Rect) {
        self.providers_tab_hit_areas = ProvidersTabHitAreas::default();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(3),
                Constraint::Length(3),
            ])
            .margin(1)
            .split(area);

        let provider_focused = self.providers_tab_focus == ProvidersTabFocus::DefaultProvider;
        let provider_text = format!(
            "{} ▼",
            if self.default_provider.is_empty() {
                "Select provider".to_string()
            } else {
                self.default_provider.clone()
            }
        );
        let provider_widget = Paragraph::new(provider_text)
            .block(
                Block::default()
                    .title(" Default Provider ")
                    .borders(Borders::ALL)
                    .border_style(if provider_focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            )
            .style(if provider_focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            });
        self.providers_tab_hit_areas.default_provider = Some(chunks[0]);
        f.render_widget(provider_widget, chunks[0]);

        let model_focused = self.providers_tab_focus == ProvidersTabFocus::DefaultModel;
        let model_text = format!(
            "{} ▼",
            if self.default_model.is_empty() {
                "Select model".to_string()
            } else {
                self.default_model.clone()
            }
        );
        let model_widget = Paragraph::new(model_text)
            .block(
                Block::default()
                    .title(" Default Model ")
                    .borders(Borders::ALL)
                    .border_style(if model_focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            )
            .style(if model_focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            });
        self.providers_tab_hit_areas.default_model = Some(chunks[1]);
        f.render_widget(model_widget, chunks[1]);

        // Button Row: [Grab Keys] [Add provider] [Edit] [Reload]
        let button_row = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(16),
                Constraint::Length(18),
                Constraint::Length(20),
                Constraint::Length(20),
                Constraint::Min(0),
            ])
            .split(chunks[2]);

        let env_focused = self.providers_tab_focus == ProvidersTabFocus::UseEnvToggle;
        let env_button = Paragraph::new(" Grab keys ")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(if env_focused {
                        Style::default().fg(Color::Magenta)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    })
                    .style(if env_focused {
                        Style::default().bg(Color::Magenta)
                    } else {
                        Style::default()
                    }),
            )
            .style(if env_focused {
                Style::default()
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Magenta)
            })
            .alignment(Alignment::Center);
        self.providers_tab_hit_areas.grab_env_button = Some(button_row[0]);
        f.render_widget(env_button, button_row[0]);

        let add_focused = self.providers_tab_focus == ProvidersTabFocus::AddProviderButton;
        let add_button = Paragraph::new(" Add provider ")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(if add_focused {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    })
                    .style(if add_focused {
                        Style::default().bg(Color::Green)
                    } else {
                        Style::default()
                    }),
            )
            .style(if add_focused {
                Style::default()
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            })
            .alignment(Alignment::Center);
        self.providers_tab_hit_areas.add_button = Some(button_row[1]);
        f.render_widget(add_button, button_row[1]);

        let edit_focused = self.providers_tab_focus == ProvidersTabFocus::EditProvidersButton;
        let edit_button = Paragraph::new(" Edit providers ")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(if edit_focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    })
                    .style(if edit_focused {
                        Style::default().bg(Color::Cyan)
                    } else {
                        Style::default()
                    }),
            )
            .style(if edit_focused {
                Style::default()
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            })
            .alignment(Alignment::Center);
        self.providers_tab_hit_areas.edit_button = Some(button_row[2]);
        f.render_widget(edit_button, button_row[2]);

        let reload_focused = self.providers_tab_focus == ProvidersTabFocus::ReloadModelsButton;
        let reload_button = Paragraph::new(" Reload Models ")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(if reload_focused {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    })
                    .style(if reload_focused {
                        Style::default().bg(Color::Yellow)
                    } else {
                        Style::default()
                    }),
            )
            .style(if reload_focused {
                Style::default()
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Yellow)
            })
            .alignment(Alignment::Center);
        self.providers_tab_hit_areas.reload_models_button = Some(button_row[3]);
        f.render_widget(reload_button, button_row[3]);

        if let Some(dropdown) = self.providers_dropdown_open {
            if dropdown == ProvidersDropdown::DefaultModel
                || dropdown == ProvidersDropdown::SmallModel
            {
                self.render_model_list_inline(f, chunks[3], dropdown);
                let explanation = Paragraph::new(vec![Line::from(Span::styled(
                    "Use Up/Down to scroll, Enter to select, Esc or click outside to close.",
                    Style::default().fg(Color::DarkGray),
                ))])
                .alignment(Alignment::Center);
                f.render_widget(explanation, chunks[4]);
                return;
            }
        }

        let saved_block = Block::default()
            .title(" Saved Providers ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        let saved_inner = saved_block.inner(chunks[3]);
        f.render_widget(saved_block, chunks[3]);

        let mut row_idx = 0;

        let oauth_providers = self.oauth_providers();
        for (name, _, _, _, _) in &oauth_providers {
            if row_idx >= saved_inner.height as usize {
                break;
            }
            let row_area = Rect::new(
                saved_inner.x,
                saved_inner.y + row_idx as u16,
                saved_inner.width,
                1,
            );
            let enabled = !self.disabled_providers.contains(name);
            let toggle = if enabled { "[✓]" } else { "[ ]" };
            let connected = self.has_saved_key(name);
            let color = if enabled && connected {
                Color::Green
            } else if !enabled {
                Color::DarkGray
            } else {
                Color::Yellow
            };
            let is_focused = self.providers_tab_focus == ProvidersTabFocus::OAuthProvider(row_idx);
            self.providers_tab_hit_areas.oauth_rows.push(row_area);
            f.render_widget(
                Paragraph::new(format!("{} {}", toggle, name)).style(if is_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(color)
                }),
                row_area,
            );
            row_idx += 1;
        }

        let preset_providers = self.preset_api_key_providers();
        for (idx, (name, _, _, _, _)) in preset_providers.iter().enumerate() {
            if row_idx >= saved_inner.height as usize {
                break;
            }
            let row_area = Rect::new(
                saved_inner.x,
                saved_inner.y + row_idx as u16,
                saved_inner.width,
                1,
            );
            let enabled = !self.disabled_providers.contains(name);
            let toggle = if enabled { "[✓]" } else { "[ ]" };
            let connected = self.has_saved_key(name);
            let color = if enabled && connected {
                Color::Green
            } else if !enabled {
                Color::DarkGray
            } else {
                Color::Yellow
            };
            let is_focused = self.providers_tab_focus == ProvidersTabFocus::PresetProvider(idx);
            self.providers_tab_hit_areas.preset_rows.push(row_area);
            f.render_widget(
                Paragraph::new(format!("{} {}", toggle, name)).style(if is_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(color)
                }),
                row_area,
            );
            row_idx += 1;
        }

        let custom_providers: Vec<_> = self
            .db_providers
            .iter()
            .filter(|(n, _, _, _, auth_type)| {
                auth_type != "oauth" && preset_providers.iter().all(|(pn, _, _, _, _)| pn != n)
            })
            .collect();
        for (idx, (name, _, _, _, _)) in custom_providers.iter().enumerate() {
            if row_idx >= saved_inner.height as usize {
                break;
            }
            let row_area = Rect::new(
                saved_inner.x,
                saved_inner.y + row_idx as u16,
                saved_inner.width,
                1,
            );
            let enabled = !self.disabled_providers.contains(name);
            let toggle = if enabled { "[✓]" } else { "[ ]" };
            let connected = self.has_saved_key(name);
            let color = if enabled && connected {
                Color::Green
            } else if !enabled {
                Color::DarkGray
            } else {
                Color::Yellow
            };
            let is_focused = self.providers_tab_focus == ProvidersTabFocus::SavedKeyList(idx);
            self.providers_tab_hit_areas.saved_key_rows.push(row_area);
            f.render_widget(
                Paragraph::new(format!("{} {}", toggle, name)).style(if is_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(color)
                }),
                row_area,
            );
            row_idx += 1;
        }

        let explanation = Paragraph::new(vec![
        Line::from(Span::styled(
            "Keys are read from: environment variables, .env, ~/.env, or \"api_key_<provider>\" settings.",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "Format: PROVIDER_API_KEY  (e.g. OPENAI_API_KEY, ANTHROPIC_API_KEY)",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "OAuth providers (Gemini, Codex): login via CLI first, tokens read from ~/.gemini.json, ~/.codex.json",
            Style::default().fg(Color::DarkGray),
        )),
    ]).alignment(Alignment::Center);
        f.render_widget(explanation, chunks[4]);

        self.render_providers_dropdowns(f, chunks);
    }

    pub(super) fn render_model_list_inline(
        &mut self,
        f: &mut Frame,
        area: Rect,
        dropdown: ProvidersDropdown,
    ) {
        const VISIBLE: usize = 6;
        const SB_W: u16 = 1;
        let total = self.available_models.len();
        let max_visible = VISIBLE.min(total);
        let offset = self
            .dropdown_scroll_offset
            .min(total.saturating_sub(max_visible));
        self.dropdown_scroll_offset = offset;

        let visible_models: Vec<_> = self
            .available_models
            .iter()
            .skip(offset)
            .take(max_visible)
            .collect();

        let block = Block::default()
            .title(format!(
                " {} ",
                if dropdown == ProvidersDropdown::DefaultModel {
                    "Default Model"
                } else {
                    "Small Model"
                }
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Black));
        let inner = block.inner(area);
        f.render_widget(Clear, area);
        f.render_widget(block, area);

        let list_width = inner.width - SB_W;
        for (i, m) in visible_models.iter().enumerate() {
            let row = Rect::new(inner.x, inner.y + i as u16, list_width, 1);
            let is_selected = if dropdown == ProvidersDropdown::DefaultModel {
                m.id == self.default_model
            } else {
                m.id == self.small_model_name()
            };
            let price = match (m.input_price, m.output_price) {
                (Some(inp), Some(out)) => format!("${:.2}/${:.2}", inp, out),
                (Some(inp), None) => format!("${:.2}/-", inp),
                (None, Some(out)) => format!("-/${:.2}", out),
                _ => String::new(),
            };
            let label = if price.is_empty() {
                m.id.clone()
            } else {
                format!("{:30} {}", m.id, price)
            };
            f.render_widget(
                Paragraph::new(label).style(if is_selected {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::White)
                }),
                row,
            );
            let target = if dropdown == ProvidersDropdown::DefaultModel {
                &mut self.providers_tab_hit_areas.default_model_items
            } else {
                &mut self.providers_tab_hit_areas.small_model_items
            };
            if i >= target.len() {
                target.push(row);
            } else {
                target[i] = row;
            }
        }

        if total > max_visible {
            let sb_x = inner.x + list_width;
            let sb_area = Rect::new(sb_x, inner.y, SB_W, inner.height);
            let thumb_h =
                ((max_visible as f64 / total as f64) * inner.height as f64).max(1.0) as u16;
            let thumb_y = ((offset as f64 / total as f64) * inner.height as f64) as u16;
            f.render_widget(
                Paragraph::new("").style(Style::default().bg(Color::DarkGray)),
                sb_area,
            );
            f.render_widget(
                Paragraph::new("").style(Style::default().bg(Color::White)),
                Rect::new(
                    sb_x,
                    sb_area.y + thumb_y.min(inner.height.saturating_sub(1)),
                    SB_W,
                    thumb_h,
                ),
            );
        }
    }

    pub(super) fn render_providers_dropdowns(
        &mut self,
        f: &mut Frame,
        chunks: std::rc::Rc<[Rect]>,
    ) {
        let Some(dropdown) = self.providers_dropdown_open else {
            return;
        };
        if dropdown == ProvidersDropdown::DefaultModel || dropdown == ProvidersDropdown::SmallModel
        {
            return;
        }

        const VISIBLE_ITEMS: usize = 8;
        const SCROLLBAR_WIDTH: u16 = 1;

        match dropdown {
            ProvidersDropdown::DefaultProvider | ProvidersDropdown::SmallProvider => {
                let current_provider = if dropdown == ProvidersDropdown::DefaultProvider {
                    &self.default_provider
                } else {
                    &self.small_model_provider()
                };
                let provider_names: Vec<String> = self.all_enabled_provider_names();
                let total = provider_names.len();
                let max_visible = VISIBLE_ITEMS.min(total);

                let offset = self
                    .dropdown_scroll_offset
                    .min(total.saturating_sub(max_visible));
                self.dropdown_scroll_offset = offset;

                let visible_names: Vec<_> = provider_names
                    .iter()
                    .skip(offset)
                    .take(max_visible)
                    .collect();

                let items: Vec<ListItem> = visible_names
                    .iter()
                    .map(|name| {
                        let style = if *name == current_provider {
                            Style::default().fg(Color::Black).bg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        ListItem::new(name.as_str()).style(style)
                    })
                    .collect();

                let anchor = if dropdown == ProvidersDropdown::DefaultProvider {
                    chunks[0]
                } else {
                    chunks[2]
                };
                let content_height = max_visible as u16;
                let dropdown_area = Self::dropdown_area_below(anchor, content_height + 2);

                let content_width = dropdown_area.width - 2 - SCROLLBAR_WIDTH;
                let viewport = Rect::new(
                    dropdown_area.x + 1,
                    dropdown_area.y + 1,
                    content_width,
                    content_height,
                );

                let list = List::new(items).style(Style::default().bg(Color::Black));
                f.render_widget(Clear, dropdown_area);
                f.render_widget(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan))
                        .style(Style::default().bg(Color::Black)),
                    dropdown_area,
                );
                f.render_widget(list, viewport);

                if total > max_visible {
                    let sb_x = dropdown_area.x + dropdown_area.width - 2;
                    let sb_area =
                        Rect::new(sb_x, dropdown_area.y + 1, SCROLLBAR_WIDTH, content_height);
                    let thumb_h = ((max_visible as f64 / total as f64) * content_height as f64)
                        .max(1.0) as u16;
                    let thumb_y = ((offset as f64 / total as f64) * content_height as f64) as u16;
                    let thumb = Rect::new(
                        sb_x,
                        sb_area.y + thumb_y.min(content_height.saturating_sub(1)),
                        SCROLLBAR_WIDTH,
                        thumb_h,
                    );
                    f.render_widget(
                        Paragraph::new("").style(Style::default().bg(Color::DarkGray)),
                        sb_area,
                    );
                    f.render_widget(
                        Paragraph::new("").style(Style::default().bg(Color::White)),
                        thumb,
                    );
                }

                self.providers_tab_hit_areas.default_provider_items.clear();
                for i in 0..max_visible {
                    self.providers_tab_hit_areas
                        .default_provider_items
                        .push(Rect {
                            x: viewport.x,
                            y: viewport.y + i as u16,
                            width: viewport.width,
                            height: 1,
                        });
                }
            }
            ProvidersDropdown::DefaultModel | ProvidersDropdown::SmallModel => {
                let total = self.available_models.len();
                let max_visible = VISIBLE_ITEMS.min(total);

                let offset = self
                    .dropdown_scroll_offset
                    .min(total.saturating_sub(max_visible));
                self.dropdown_scroll_offset = offset;

                let visible_models: Vec<_> = self
                    .available_models
                    .iter()
                    .skip(offset)
                    .take(max_visible)
                    .collect();

                let items: Vec<ListItem> = visible_models
                    .iter()
                    .map(|m| {
                        let is_selected = if dropdown == ProvidersDropdown::DefaultModel {
                            m.id == self.default_model
                        } else {
                            m.id == self.small_model_name()
                        };
                        let price_text = match (m.input_price, m.output_price) {
                            (Some(inp), Some(out)) => format!("${:.2}/${:.2}", inp, out),
                            (Some(inp), None) => format!("${:.2}/-", inp),
                            (None, Some(out)) => format!("-/${:.2}", out),
                            (None, None) => String::new(),
                        };
                        let label = if price_text.is_empty() {
                            m.id.clone()
                        } else {
                            format!("{:30} {}", m.id, price_text)
                        };
                        let style = if is_selected {
                            Style::default().fg(Color::Black).bg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        ListItem::new(label).style(style)
                    })
                    .collect();

                let anchor = if dropdown == ProvidersDropdown::DefaultModel {
                    chunks[1]
                } else {
                    chunks[3]
                };
                let content_height = max_visible as u16;
                let dropdown_area = Self::dropdown_area_below(anchor, content_height + 2);

                let content_width = dropdown_area.width - 2 - SCROLLBAR_WIDTH;
                let viewport = Rect::new(
                    dropdown_area.x + 1,
                    dropdown_area.y + 1,
                    content_width,
                    content_height,
                );

                let list = List::new(items).style(Style::default().bg(Color::Black));
                f.render_widget(Clear, dropdown_area);
                f.render_widget(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan))
                        .style(Style::default().bg(Color::Black)),
                    dropdown_area,
                );
                f.render_widget(list, viewport);

                if total > max_visible {
                    let sb_x = dropdown_area.x + dropdown_area.width - 2;
                    let sb_area =
                        Rect::new(sb_x, dropdown_area.y + 1, SCROLLBAR_WIDTH, content_height);
                    let thumb_h = ((max_visible as f64 / total as f64) * content_height as f64)
                        .max(1.0) as u16;
                    let thumb_y = ((offset as f64 / total as f64) * content_height as f64) as u16;
                    let thumb = Rect::new(
                        sb_x,
                        sb_area.y + thumb_y.min(content_height.saturating_sub(1)),
                        SCROLLBAR_WIDTH,
                        thumb_h,
                    );
                    f.render_widget(
                        Paragraph::new("").style(Style::default().bg(Color::DarkGray)),
                        sb_area,
                    );
                    f.render_widget(
                        Paragraph::new("").style(Style::default().bg(Color::White)),
                        thumb,
                    );
                }

                let target_vec = if dropdown == ProvidersDropdown::DefaultModel {
                    &mut self.providers_tab_hit_areas.default_model_items
                } else {
                    &mut self.providers_tab_hit_areas.small_model_items
                };
                target_vec.clear();
                for i in 0..max_visible {
                    target_vec.push(Rect {
                        x: viewport.x,
                        y: viewport.y + i as u16,
                        width: viewport.width,
                        height: 1,
                    });
                }
            }
        }
    }

    pub(super) fn render_provider_form_popup(
        f: &mut Frame,
        parent_area: Rect,
        form: &mut ProviderFormState,
    ) {
        form.hit_areas = ProviderFormHitAreas::default();
        let popup_area = Self::centered_rect_in(52, 60, parent_area);
        let block = Block::default()
            .title(form.title.as_str())
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .style(Style::default().bg(Color::Black));
        let inner = block.inner(popup_area);

        f.render_widget(Clear, popup_area);
        f.render_widget(block, popup_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .margin(1)
            .split(inner);

        let name_focused = form.focus == ProviderFormFocus::ProviderName;
        form.hit_areas.name = Some(chunks[0]);
        f.render_widget(
            Paragraph::new(form.name.clone())
                .block(
                    Block::default()
                        .title(" Provider Name ")
                        .borders(Borders::ALL)
                        .border_style(if name_focused {
                            Style::default().fg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        }),
                )
                .style(if name_focused {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::Gray)
                }),
            chunks[0],
        );

        let endpoint_focused = form.focus == ProviderFormFocus::ProviderEndpoint;
        form.hit_areas.endpoint = Some(chunks[1]);
        f.render_widget(
            Paragraph::new(form.endpoint.clone())
                .block(
                    Block::default()
                        .title(" Endpoint Base URL ")
                        .borders(Borders::ALL)
                        .border_style(if endpoint_focused {
                            Style::default().fg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        }),
                )
                .style(if endpoint_focused {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::Gray)
                }),
            chunks[1],
        );

        let backend_focused = form.focus == ProviderFormFocus::ProviderBackendType;
        form.hit_areas.backend_type = Some(chunks[2]);
        f.render_widget(
            Paragraph::new(format!("{} ▼", Self::backend_label(&form.backend_type)))
                .block(
                    Block::default()
                        .title(" SDK Backend Type ")
                        .borders(Borders::ALL)
                        .border_style(if backend_focused {
                            Style::default().fg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        }),
                )
                .style(if backend_focused {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::Gray)
                }),
            chunks[2],
        );

        let api_key_focused = form.focus == ProviderFormFocus::ProviderApiKey;
        let api_key_display = if api_key_focused {
            form.api_key.clone()
        } else {
            mask_key(&form.api_key)
        };
        form.hit_areas.api_key = Some(chunks[3]);
        f.render_widget(
            Paragraph::new(api_key_display)
                .block(
                    Block::default()
                        .title(" API Key ")
                        .borders(Borders::ALL)
                        .border_style(if api_key_focused {
                            Style::default().fg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        }),
                )
                .style(if api_key_focused {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::Gray)
                }),
            chunks[3],
        );

        let buttons = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Min(0),
            ])
            .split(chunks[4]);

        let can_submit = form.can_submit();
        let submit_focused = form.focus == ProviderFormFocus::SubmitButton;
        form.hit_areas.submit_button = Some(buttons[0]);
        f.render_widget(
            Paragraph::new(format!(" {} ", form.submit_label))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(if submit_focused {
                            Style::default().fg(Color::Green)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        }),
                )
                .style(if can_submit && submit_focused {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else if can_submit {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                })
                .alignment(Alignment::Center),
            buttons[0],
        );

        let cancel_focused = form.focus == ProviderFormFocus::CancelButton;
        form.hit_areas.cancel_button = Some(buttons[1]);
        f.render_widget(
            Paragraph::new(" Cancel ")
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(if cancel_focused {
                            Style::default().fg(Color::Red)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        }),
                )
                .style(if cancel_focused {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Red)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Red)
                })
                .alignment(Alignment::Center),
            buttons[1],
        );

        if form.dropdown_open {
            let count = BACKEND_TYPE_OPTIONS.len() as u16;
            let dropdown_area = Rect::new(
                chunks[2].x,
                chunks[2].y + chunks[2].height,
                chunks[2].width,
                count + 2,
            );
            let items: Vec<ListItem> = BACKEND_TYPE_OPTIONS
                .iter()
                .map(|backend| {
                    let style = if *backend == form.backend_type {
                        Style::default().fg(Color::Black).bg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(Self::backend_label(backend)).style(style)
                })
                .collect();
            f.render_widget(Clear, dropdown_area);
            f.render_widget(
                List::new(items).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan))
                        .style(Style::default().bg(Color::Black)),
                ),
                dropdown_area,
            );
            for i in 0..count {
                form.hit_areas.dropdown_items.push(Rect::new(
                    dropdown_area.x + 1,
                    dropdown_area.y + 1 + i,
                    dropdown_area.width - 2,
                    1,
                ));
            }
        }
    }

    pub(super) fn render_edit_providers_popup(
        f: &mut Frame,
        parent_area: Rect,
        providers: &[EditableProvider],
        popup: &mut EditProvidersPopupState,
    ) {
        popup.hit_areas = EditProvidersHitAreas::default();

        let popup_area = Self::centered_rect_in(48, 60, parent_area);
        let block = Block::default()
            .title(" Edit Providers ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .style(Style::default().bg(Color::Black));
        let inner = block.inner(popup_area);
        f.render_widget(Clear, popup_area);
        f.render_widget(block, popup_area);

        if providers.is_empty() {
            f.render_widget(
                Paragraph::new("No providers saved yet.")
                    .style(Style::default().fg(Color::DarkGray))
                    .alignment(Alignment::Center),
                inner,
            );
            return;
        }

        let max_visible = inner.height as usize;
        for (i, provider) in providers.iter().enumerate().take(max_visible) {
            let row_y = inner.y + i as u16;
            let content_width = inner.width.saturating_sub(5);
            let name_area = Rect::new(inner.x, row_y, content_width, 1);
            let delete_area = Rect::new(inner.x + content_width, row_y, 4, 1);
            let name_focused = popup.focus == Some(EditProvidersFocus::ProviderName(i));
            let delete_focused = popup.focus == Some(EditProvidersFocus::DeleteButton(i));

            f.render_widget(
                Paragraph::new(provider.name.clone()).style(if name_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                }),
                name_area,
            );
            f.render_widget(
                Paragraph::new("[X]").style(if delete_focused {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Red)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Red)
                }),
                delete_area,
            );

            popup.hit_areas.provider_rows.push((name_area, delete_area));
        }
    }

    pub fn handle_provider_popup_click(&mut self, pos: Position) -> ProvidersAction {
        if self.preset_key_popup.is_some() {
            return self.handle_preset_key_popup_click(pos);
        }
        if let Some(form) = self.edit_provider_popup.as_mut() {
            if let Some(action) = Self::handle_form_click(form, pos) {
                return match action {
                    FormClickAction::Activate => self.activate_form_popup(true),
                    FormClickAction::Noop => ProvidersAction::None,
                };
            }
            return ProvidersAction::None;
        }
        if let Some(popup) = self.edit_providers_popup.as_mut() {
            for (idx, (name_area, delete_area)) in popup.hit_areas.provider_rows.iter().enumerate()
            {
                if name_area.contains(pos) {
                    popup.focus = Some(EditProvidersFocus::ProviderName(idx));
                    self.open_edit_provider_popup(idx);
                    return ProvidersAction::None;
                }
                if delete_area.contains(pos) {
                    popup.focus = Some(EditProvidersFocus::DeleteButton(idx));
                    if let Some(provider) = self.providers_tab_list.get(idx) {
                        return ProvidersAction::DeleteProvider(provider.name.clone());
                    }
                }
            }
            return ProvidersAction::None;
        }
        if let Some(form) = self.add_provider_popup.as_mut() {
            if let Some(action) = Self::handle_form_click(form, pos) {
                return match action {
                    FormClickAction::Activate => self.activate_form_popup(false),
                    FormClickAction::Noop => ProvidersAction::None,
                };
            }
            return ProvidersAction::None;
        }
        ProvidersAction::None
    }

    fn handle_form_click(form: &mut ProviderFormState, pos: Position) -> Option<FormClickAction> {
        if form.dropdown_open {
            for (idx, area) in form.hit_areas.dropdown_items.iter().enumerate() {
                if area.contains(pos) {
                    form.backend_type = BACKEND_TYPE_OPTIONS[idx].to_string();
                    form.dropdown_open = false;
                    form.focus = ProviderFormFocus::ProviderBackendType;
                    return Some(FormClickAction::Noop);
                }
            }
        }
        if let Some(area) = form.hit_areas.name {
            if area.contains(pos) {
                form.focus = ProviderFormFocus::ProviderName;
                form.dropdown_open = false;
                return Some(FormClickAction::Noop);
            }
        }
        if let Some(area) = form.hit_areas.endpoint {
            if area.contains(pos) {
                form.focus = ProviderFormFocus::ProviderEndpoint;
                form.dropdown_open = false;
                return Some(FormClickAction::Noop);
            }
        }
        if let Some(area) = form.hit_areas.backend_type {
            if area.contains(pos) {
                form.focus = ProviderFormFocus::ProviderBackendType;
                form.dropdown_open = !form.dropdown_open;
                return Some(FormClickAction::Noop);
            }
        }
        if let Some(area) = form.hit_areas.api_key {
            if area.contains(pos) {
                form.focus = ProviderFormFocus::ProviderApiKey;
                form.dropdown_open = false;
                return Some(FormClickAction::Noop);
            }
        }
        if let Some(area) = form.hit_areas.submit_button {
            if area.contains(pos) {
                form.focus = ProviderFormFocus::SubmitButton;
                return Some(FormClickAction::Activate);
            }
        }
        if let Some(area) = form.hit_areas.cancel_button {
            if area.contains(pos) {
                form.focus = ProviderFormFocus::CancelButton;
                return Some(FormClickAction::Activate);
            }
        }
        None
    }

    pub fn open_add_provider_popup(&mut self) {
        self.add_provider_popup = Some(ProviderFormState::new_add());
    }

    pub fn open_edit_providers_popup(&mut self) {
        self.edit_providers_popup = Some(EditProvidersPopupState::new(
            !self.providers_tab_list.is_empty(),
        ));
    }

    pub fn open_edit_provider_popup(&mut self, idx: usize) {
        if let Some(provider) = self.providers_tab_list.get(idx) {
            let api_key = self
                .saved_keys
                .iter()
                .find(|(name, _)| name == &provider.name)
                .map(|(_, key)| key.clone())
                .unwrap_or_default();
            self.edit_provider_popup = Some(ProviderFormState::new_edit(provider, api_key));
        }
    }

    pub fn open_preset_key_popup(&mut self, idx: usize) {
        if let Some((name, endpoint, _, _, _)) = self.preset_api_key_providers().get(idx) {
            let api_key = self
                .saved_keys
                .iter()
                .find(|(provider_name, _)| provider_name == name)
                .map(|(_, key)| key.clone())
                .unwrap_or_default();
            self.preset_key_popup = Some(PresetKeyPopupState::new(
                name.clone(),
                endpoint.clone(),
                api_key,
            ));
            self.providers_tab_focus = ProvidersTabFocus::PopupApiKey;
        }
    }

    pub fn apply_add_provider(&mut self, provider: EditableProvider, api_key: String) {
        let env_var = Self::env_var_for_name(&provider.name);
        self.db_providers.push((
            provider.name.clone(),
            provider.endpoint.clone(),
            env_var,
            provider.backend_type.clone(),
            "api_key".to_string(),
        ));
        self.providers_tab_list.push(provider.clone());
        self.providers_tab_list.sort_by(|a, b| a.name.cmp(&b.name));
        self.db_providers.sort_by(|a, b| a.0.cmp(&b.0));
        self.set_saved_key(provider.name, api_key);
        self.add_provider_popup = None;
    }

    pub fn apply_update_provider(
        &mut self,
        original_name: &str,
        provider: EditableProvider,
        api_key: String,
    ) {
        if let Some(entry) = self
            .db_providers
            .iter_mut()
            .find(|(name, _, _, _, _)| name == original_name)
        {
            let auth_type = entry.4.clone();
            *entry = (
                provider.name.clone(),
                provider.endpoint.clone(),
                Self::env_var_for_name(&provider.name),
                provider.backend_type.clone(),
                auth_type,
            );
        }
        if let Some(editable) = self
            .providers_tab_list
            .iter_mut()
            .find(|existing| existing.name == original_name)
        {
            *editable = provider.clone();
        }
        self.providers_tab_list.sort_by(|a, b| a.name.cmp(&b.name));
        self.db_providers.sort_by(|a, b| a.0.cmp(&b.0));

        self.saved_keys.retain(|(name, _)| name != original_name);
        self.set_saved_key(provider.name.clone(), api_key);
        if self.default_provider == original_name {
            self.default_provider = provider.name;
            self.default_model.clear();
        }
        self.edit_provider_popup = None;
    }

    pub fn apply_preset_key_save(&mut self, provider_name: String, api_key: String) {
        self.set_saved_key(provider_name, api_key);
        self.preset_key_popup = None;
    }

    pub fn remove_provider_by_name(&mut self, name: &str) {
        self.db_providers
            .retain(|(provider_name, _, _, _, _)| provider_name != name);
        self.providers_tab_list
            .retain(|provider| provider.name != name);
        self.saved_keys
            .retain(|(provider_name, _)| provider_name != name);

        if self.default_provider == name {
            self.default_provider = self
                .saved_keys
                .first()
                .map(|(provider_name, _)| provider_name.clone())
                .unwrap_or_default();
            self.default_model.clear();
        }

        if let Some(popup) = self.edit_providers_popup.as_mut() {
            let len = self.providers_tab_list.len();
            popup.focus = if len == 0 {
                None
            } else {
                match popup.focus.unwrap_or(EditProvidersFocus::ProviderName(0)) {
                    EditProvidersFocus::ProviderName(idx) => {
                        Some(EditProvidersFocus::ProviderName(idx.min(len - 1)))
                    }
                    EditProvidersFocus::DeleteButton(idx) => {
                        Some(EditProvidersFocus::DeleteButton(idx.min(len - 1)))
                    }
                }
            };
        }

        if self
            .edit_provider_popup
            .as_ref()
            .and_then(|form| form.original_name.as_ref())
            .map(|original| original == name)
            .unwrap_or(false)
        {
            self.edit_provider_popup = None;
        }
    }

    pub(super) fn set_saved_key(&mut self, provider: String, api_key: String) {
        self.saved_keys.retain(|(name, _)| name != &provider);
        if !api_key.is_empty() {
            self.saved_keys.push((provider, api_key));
            self.saved_keys.sort_by(|a, b| a.0.cmp(&b.0));
        }
    }

    pub fn oauth_providers(&self) -> Vec<ProviderEntry> {
        self.db_providers
            .iter()
            .filter(|(_, _, _, _, auth_type)| auth_type == "oauth")
            .cloned()
            .collect()
    }

    pub fn preset_api_key_providers(&self) -> Vec<ProviderEntry> {
        let mut providers: Vec<ProviderEntry> = self
            .db_providers
            .iter()
            .filter(|(name, _, _, _, auth_type)| {
                auth_type == "api_key" && PRESET_PROVIDER_NAMES.contains(&name.as_str())
            })
            .cloned()
            .collect();
        providers.extend(
            SEARCH_KEY_PROVIDERS
                .iter()
                .map(|(name, endpoint, env_var)| {
                    (
                        (*name).to_string(),
                        (*endpoint).to_string(),
                        (*env_var).to_string(),
                        "search".to_string(),
                        "api_key".to_string(),
                    )
                }),
        );
        providers
    }

    pub(super) fn has_saved_key(&self, provider_name: &str) -> bool {
        self.saved_keys
            .iter()
            .any(|(name, key)| name == provider_name && !key.trim().is_empty())
    }

    pub(super) fn env_var_for_name(name: &str) -> String {
        format!("{}_API_KEY", name.to_uppercase().replace(' ', "_"))
    }

    pub fn small_model_provider(&self) -> String {
        self.small_model.split(':').next().unwrap_or("").to_string()
    }

    pub fn small_model_name(&self) -> String {
        self.small_model.split(':').nth(1).unwrap_or("").to_string()
    }

    pub fn small_model_tuple(&self) -> Option<(String, String, String, String, String, String)> {
        let prov = self.small_model_provider();
        let model = self.small_model_name();
        if prov.is_empty() {
            return None;
        }
        let (endpoint, env_var, backend_type, _auth) = self
            .db_providers
            .iter()
            .find(|(n, _, _, _, _)| n == &prov)
            .map(|(_, ep, ev, bt, au)| (ep.clone(), ev.clone(), bt.clone(), au.clone()))
            .unwrap_or_default();
        Some((prov.clone(), endpoint, env_var, backend_type, prov, model))
    }

    pub fn toggle_providers_dropdown(&mut self, dropdown: ProvidersDropdown) {
        if self.providers_dropdown_open == Some(dropdown) {
            self.providers_dropdown_open = None;
        } else {
            self.providers_dropdown_open = Some(dropdown);
            self.dropdown_scroll_offset = 0;
        }
    }

    pub fn select_providers_dropdown_item(&mut self, idx: usize) {
        let real_idx = idx + self.dropdown_scroll_offset;
        let provider_names = self.all_enabled_provider_names();
        match self.providers_dropdown_open {
            Some(ProvidersDropdown::DefaultProvider) => {
                if real_idx < provider_names.len() {
                    self.default_provider = provider_names[real_idx].clone();
                    self.default_model.clear();
                }
            }
            Some(ProvidersDropdown::SmallProvider) => {
                if real_idx < provider_names.len() {
                    let current_model = self.small_model_name();
                    let new_prov = &provider_names[real_idx];
                    self.small_model = format!("{}:{}", new_prov, current_model);
                }
            }
            Some(ProvidersDropdown::DefaultModel) => {
                if let Some(model) = self.available_models.get(real_idx) {
                    self.default_model = model.id.clone();
                }
            }
            Some(ProvidersDropdown::SmallModel) => {
                if let Some(model) = self.available_models.get(real_idx) {
                    let current_prov = self.small_model_provider();
                    self.small_model = format!("{}:{}", current_prov, model.id);
                }
            }
            None => {}
        }
        self.providers_dropdown_open = None;
        self.dropdown_scroll_offset = 0;
    }

    pub fn providers_dropdown_up(&mut self) {
        if self.dropdown_scroll_offset > 0 {
            self.dropdown_scroll_offset -= 1;
        }
    }

    pub fn providers_dropdown_down(&mut self) {
        let total = match self.providers_dropdown_open {
            Some(ProvidersDropdown::DefaultProvider) | Some(ProvidersDropdown::SmallProvider) => {
                self.all_enabled_provider_names().len()
            }
            Some(ProvidersDropdown::DefaultModel) | Some(ProvidersDropdown::SmallModel) => {
                self.available_models.len()
            }
            None => return,
        };
        let max_visible = 8.min(total);
        let max_offset = total.saturating_sub(max_visible);
        if self.dropdown_scroll_offset < max_offset {
            self.dropdown_scroll_offset += 1;
        }
    }

    pub fn handle_providers_click(&mut self, pos: Position) -> ProvidersAction {
        if let Some(area) = self.providers_tab_hit_areas.popup_api_key {
            if area.contains(pos) {
                self.providers_tab_focus = ProvidersTabFocus::PopupApiKey;
                return ProvidersAction::None;
            }
        }
        if let Some(area) = self.providers_tab_hit_areas.popup_save {
            if area.contains(pos) {
                self.providers_tab_focus = ProvidersTabFocus::PopupSaveButton;
                return self.activate_preset_key_popup();
            }
        }
        if let Some(area) = self.providers_tab_hit_areas.popup_cancel {
            if area.contains(pos) {
                self.providers_tab_focus = ProvidersTabFocus::PopupCancelButton;
                self.preset_key_popup = None;
                return ProvidersAction::None;
            }
        }
        ProvidersAction::None
    }

    pub fn grab_keys_from_env(&mut self) {
        for (provider, _, var_name, _, auth_type) in &self.db_providers {
            if auth_type == "oauth" {
                continue;
            }
            if let Ok(val) = std::env::var(var_name) {
                if !val.is_empty() {
                    self.saved_keys.retain(|(p, _)| p != provider);
                    self.saved_keys.push((provider.clone(), val));
                }
            }
        }
        for (provider, _, var_name) in SEARCH_KEY_PROVIDERS {
            if let Ok(val) = std::env::var(var_name) {
                if !val.is_empty() {
                    self.saved_keys.retain(|(p, _)| p != provider);
                    self.saved_keys.push(((*provider).to_string(), val));
                }
            }
        }

        let home = dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        for path in [".env".to_string(), format!("{}/.env", home)] {
            if let Ok(content) = std::fs::read_to_string(&path) {
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    for (provider, _, var_name, _, auth_type) in &self.db_providers {
                        if auth_type == "oauth" {
                            continue;
                        }
                        let prefix = format!("{}=", var_name);
                        if let Some(val) = line.strip_prefix(&prefix) {
                            let val = val.trim().trim_matches('"').trim_matches('\'').to_string();
                            if !val.is_empty() {
                                self.saved_keys.retain(|(p, _)| p != provider);
                                self.saved_keys.push((provider.clone(), val));
                            }
                        }
                    }
                    for (provider, _, var_name) in SEARCH_KEY_PROVIDERS {
                        let prefix = format!("{}=", var_name);
                        if let Some(val) = line.strip_prefix(&prefix) {
                            let val = val.trim().trim_matches('"').trim_matches('\'').to_string();
                            if !val.is_empty() {
                                self.saved_keys.retain(|(p, _)| p != provider);
                                self.saved_keys.push(((*provider).to_string(), val));
                            }
                        }
                    }
                }
            }
        }

        self.check_oauth_tokens();
        self.saved_keys.sort_by(|a, b| a.0.cmp(&b.0));
    }

    pub fn check_oauth_tokens(&mut self) -> Vec<String> {
        let mut found = Vec::new();

        for provider_name in self
            .oauth_providers()
            .into_iter()
            .map(|(name, _, _, _, _)| name)
        {
            if crate::llm::auth::read_oauth_token(&provider_name).is_some() {
                self.saved_keys.retain(|(name, _)| name != &provider_name);
                self.saved_keys
                    .push((provider_name.to_string(), "OAuth token found".to_string()));
                found.push(provider_name.to_string());
            }
        }

        found
    }

    pub fn provider_endpoint(&self, provider_name: &str) -> Option<String> {
        self.db_providers
            .iter()
            .find(|(name, _, _, _, _)| name == provider_name)
            .map(|(_, endpoint, _, _, _)| endpoint.clone())
    }

    pub fn move_providers_tab_focus(&mut self, forward: bool) {
        if self.providers_dropdown_open.is_some() {
            if forward {
                self.providers_dropdown_down();
            } else {
                self.providers_dropdown_up();
            }
            return;
        }
        let order = [
            ProvidersTabFocus::DefaultProvider,
            ProvidersTabFocus::DefaultModel,
            ProvidersTabFocus::UseEnvToggle,
            ProvidersTabFocus::AddProviderButton,
            ProvidersTabFocus::EditProvidersButton,
            ProvidersTabFocus::ReloadModelsButton,
        ];
        let current_idx = order.iter().position(|f| *f == self.providers_tab_focus);
        let new_focus = match current_idx {
            Some(idx) if forward => {
                let next = (idx + 1) % order.len();
                order[next]
            }
            Some(idx) => {
                let prev = if idx == 0 { order.len() - 1 } else { idx - 1 };
                order[prev]
            }
            None => ProvidersTabFocus::DefaultProvider,
        };
        self.providers_tab_focus = new_focus;
    }

    pub(super) fn activate_preset_key_popup(&mut self) -> ProvidersAction {
        let Some(popup) = &self.preset_key_popup else {
            return ProvidersAction::None;
        };

        match self.providers_tab_focus {
            ProvidersTabFocus::PopupSaveButton if popup.can_submit() => {
                let provider_name = popup.provider_name.clone();
                let api_key = popup.api_key.trim().to_string();
                self.preset_key_popup = None;
                ProvidersAction::SavePresetKey {
                    provider_name,
                    api_key,
                }
            }
            ProvidersTabFocus::PopupCancelButton => {
                self.preset_key_popup = None;
                ProvidersAction::None
            }
            _ => ProvidersAction::None,
        }
    }

    pub(super) fn handle_preset_key_popup_click(&mut self, pos: Position) -> ProvidersAction {
        if let Some(area) = self.providers_tab_hit_areas.popup_api_key {
            if area.contains(pos) {
                self.providers_tab_focus = ProvidersTabFocus::PopupApiKey;
                return ProvidersAction::None;
            }
        }
        if let Some(area) = self.providers_tab_hit_areas.popup_save {
            if area.contains(pos) {
                self.providers_tab_focus = ProvidersTabFocus::PopupSaveButton;
                return self.activate_preset_key_popup();
            }
        }
        if let Some(area) = self.providers_tab_hit_areas.popup_cancel {
            if area.contains(pos) {
                self.providers_tab_focus = ProvidersTabFocus::PopupCancelButton;
                self.preset_key_popup = None;
                return ProvidersAction::None;
            }
        }
        ProvidersAction::None
    }
}

enum FormClickAction {
    Noop,
    Activate,
}

pub(super) fn mask_key(key: &str) -> String {
    if key.is_empty() {
        String::new()
    } else if key.len() > 8 {
        format!("{}...{}", &key[..4], &key[key.len() - 4..])
    } else {
        "••••".to_string()
    }
}
