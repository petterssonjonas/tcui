use super::*;

impl SettingsPopup {
    pub(super) fn render_models(&mut self, f: &mut Frame, area: Rect) {
        self.models_tab_hit_areas = ModelsTabHitAreas::default();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(2),
            ])
            .margin(1)
            .split(area);

        let provider_focused = self.models_tab_focus == ModelsTabFocus::Provider;
        let provider_widget = Paragraph::new(format!("{} ▼", self.models_provider))
            .block(
                Block::default()
                    .title(" Provider ")
                    .borders(Borders::ALL)
                    .border_style(if provider_focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            )
            .style(if provider_focused {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            });
        self.models_tab_hit_areas.provider = Some(chunks[0]);
        f.render_widget(provider_widget, chunks[0]);

        let list_block = Block::default()
            .title(" Models For Selected Provider ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        let list_inner = list_block.inner(chunks[1]);
        f.render_widget(list_block, chunks[1]);

        for (idx, model) in self.models_available_models.iter().enumerate() {
            if idx >= list_inner.height as usize {
                break;
            }
            let row_area = Rect::new(list_inner.x, list_inner.y + idx as u16, list_inner.width, 1);
            let enabled = !self
                .disabled_models
                .contains(&Self::disabled_model_key(&self.models_provider, &model.id));
            let toggle = if enabled { "[✓]" } else { "[ ]" };
            let focused = self.models_tab_focus == ModelsTabFocus::Model(idx);
            self.models_tab_hit_areas.model_rows.push(row_area);
            f.render_widget(
                Paragraph::new(format!("{} {}", toggle, model.id)).style(if focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else if enabled {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                }),
                row_area,
            );
        }

        let help =
            Paragraph::new("Toggle providers/models here to hide them from the chat selectors.")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray));
        f.render_widget(help, chunks[2]);

        self.render_models_dropdown(f, chunks[0]);
    }

    pub(super) fn render_models_dropdown(&mut self, f: &mut Frame, anchor: Rect) {
        if !self.models_dropdown_open {
            return;
        }
        const VISIBLE_ITEMS: usize = 8;
        const SCROLLBAR_WIDTH: u16 = 1;
        let provider_names = self.all_enabled_provider_names();
        let total = provider_names.len();
        let max_visible = VISIBLE_ITEMS.min(total);
        let offset = self
            .models_dropdown_scroll_offset
            .min(total.saturating_sub(max_visible));
        self.models_dropdown_scroll_offset = offset;
        let visible_names: Vec<_> = provider_names
            .iter()
            .skip(offset)
            .take(max_visible)
            .collect();
        let items: Vec<ListItem> = visible_names
            .iter()
            .map(|name| {
                let style = if *name == &self.models_provider {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(name.as_str()).style(style)
            })
            .collect();
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
        self.models_tab_hit_areas.provider_items.clear();
        for i in 0..max_visible {
            self.models_tab_hit_areas.provider_items.push(Rect {
                x: viewport.x,
                y: viewport.y + i as u16,
                width: viewport.width,
                height: 1,
            });
        }
    }

    pub fn all_enabled_provider_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .db_providers
            .iter()
            .filter(|(name, _, _, _, _)| !self.disabled_providers.contains(name))
            .map(|(name, _, _, _, _)| name.clone())
            .collect();
        names.sort();
        names
    }

    pub fn disabled_model_key(provider: &str, model: &str) -> String {
        format!("{provider}:{model}")
    }

    pub fn toggle_models_dropdown(&mut self) {
        self.models_dropdown_open = !self.models_dropdown_open;
        if self.models_dropdown_open {
            self.models_dropdown_scroll_offset = 0;
        }
    }

    pub fn select_models_provider_dropdown_item(&mut self, idx: usize) {
        let real_idx = idx + self.models_dropdown_scroll_offset;
        let provider_names = self.all_enabled_provider_names();
        if let Some(provider) = provider_names.get(real_idx) {
            self.models_provider = provider.clone();
            self.models_tab_focus = ModelsTabFocus::Provider;
        }
        self.models_dropdown_open = false;
        self.models_dropdown_scroll_offset = 0;
    }

    pub fn models_dropdown_up(&mut self) {
        if self.models_dropdown_scroll_offset > 0 {
            self.models_dropdown_scroll_offset -= 1;
        }
    }

    pub fn models_dropdown_down(&mut self) {
        let total = self.all_enabled_provider_names().len();
        let max_visible = 8.min(total);
        let max_offset = total.saturating_sub(max_visible);
        if self.models_dropdown_scroll_offset < max_offset {
            self.models_dropdown_scroll_offset += 1;
        }
    }

    pub fn move_models_tab_focus(&mut self, forward: bool) {
        if self.models_dropdown_open {
            if forward {
                self.models_dropdown_down();
            } else {
                self.models_dropdown_up();
            }
            return;
        }
        let count = self.models_available_models.len();
        self.models_tab_focus = match self.models_tab_focus {
            ModelsTabFocus::Provider if forward && count > 0 => ModelsTabFocus::Model(0),
            ModelsTabFocus::Provider => ModelsTabFocus::Provider,
            ModelsTabFocus::Model(idx) if forward && idx + 1 < count => {
                ModelsTabFocus::Model(idx + 1)
            }
            ModelsTabFocus::Model(_) if forward => ModelsTabFocus::Provider,
            ModelsTabFocus::Model(0) => ModelsTabFocus::Provider,
            ModelsTabFocus::Model(idx) => ModelsTabFocus::Model(idx - 1),
        };
    }

    pub fn activate_models_focus(&mut self) {
        match self.models_tab_focus {
            ModelsTabFocus::Provider => self.toggle_models_dropdown(),
            ModelsTabFocus::Model(idx) => {
                if let Some(model) = self.models_available_models.get(idx) {
                    let key = Self::disabled_model_key(&self.models_provider, &model.id);
                    if self.disabled_models.contains(&key) {
                        self.disabled_models.remove(&key);
                    } else {
                        self.disabled_models.insert(key);
                    }
                }
            }
        }
    }
}
