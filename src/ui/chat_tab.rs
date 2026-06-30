#![allow(dead_code)]
use crate::config::app_config::{MarkdownMode, TextAlignment};
use crate::ui::components::image_block::{is_local_image_source, ImageBlockState};
use crate::ui::components::markdown::MarkdownRenderer;
use crate::ui::components::markdown_model::{LinkTarget, RenderedImage};
use crate::ui::components::terminal_capabilities::KittyTextOverlay;
use crate::ui::settings_tab::ModelInfo;
use ratatui::{layout::Rect, prelude::*, widgets::*, Frame};
use unicode_width::UnicodeWidthStr;

pub struct ChatTab<'a> {
    pub state: &'a mut crate::ui::ChatTabState,
    pub user_alignment: TextAlignment,
    pub ai_alignment: TextAlignment,
    pub markdown_mode: MarkdownMode,
    pub collapse_thinking: bool,
    pub show_chat_scrollbar: bool,
    pub kitty_enhanced_text: bool,
    pub kitty_text_max_scale: u8,
    pub image_protocol: &'a str,
    pub terminal_capabilities: crate::ui::components::terminal_capabilities::TerminalCapabilities,
    pub frame_tick: u64,
    pub providers: &'a [(String, String, String, String, String)],
    pub models: &'a [ModelInfo],
    pub reasoning_options: &'a [String],
}

pub struct ChatTabProps<'a> {
    pub user_alignment: TextAlignment,
    pub ai_alignment: TextAlignment,
    pub markdown_mode: MarkdownMode,
    pub collapse_thinking: bool,
    pub show_chat_scrollbar: bool,
    pub kitty_enhanced_text: bool,
    pub kitty_text_max_scale: u8,
    pub image_protocol: &'a str,
    pub terminal_capabilities: crate::ui::components::terminal_capabilities::TerminalCapabilities,
    pub frame_tick: u64,
    pub providers: &'a [(String, String, String, String, String)],
    pub models: &'a [ModelInfo],
    pub reasoning_options: &'a [String],
}

struct RenderedMessages {
    lines: Vec<Line<'static>>,
    thinking_toggle_lines: Vec<(usize, usize)>,
    link_targets: Vec<LinkTarget>,
    images: Vec<RenderedImage>,
    kitty_headings: Vec<RenderedKittyHeading>,
    answer_anchor_lines: Vec<(usize, usize)>,
}

struct RenderedKittyHeading {
    line: usize,
    text: String,
    scale: u8,
    alignment: Alignment,
}

impl<'a> ChatTab<'a> {
    pub fn new(state: &'a mut crate::ui::ChatTabState, props: ChatTabProps<'a>) -> Self {
        Self {
            state,
            user_alignment: props.user_alignment,
            ai_alignment: props.ai_alignment,
            markdown_mode: props.markdown_mode,
            collapse_thinking: props.collapse_thinking,
            show_chat_scrollbar: props.show_chat_scrollbar,
            kitty_enhanced_text: props.kitty_enhanced_text,
            kitty_text_max_scale: props.kitty_text_max_scale,
            image_protocol: props.image_protocol,
            terminal_capabilities: props.terminal_capabilities,
            frame_tick: props.frame_tick,
            providers: props.providers,
            models: props.models,
            reasoning_options: props.reasoning_options,
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        let is_empty = self.state.messages.is_empty();

        let has_title = self.state.generated_title.is_some();
        let header_lines = if has_title { 2 } else { 0 };

        let chunks = if is_empty {
            let mut constraints = vec![];
            if header_lines > 0 {
                constraints.push(Constraint::Length(header_lines));
            }
            constraints.push(Constraint::Min(0));
            constraints.push(Constraint::Length(5));
            constraints.push(Constraint::Min(0));

            Layout::default()
                .direction(Direction::Vertical)
                .constraints(constraints)
                .split(area)
        } else {
            let mut constraints = vec![];
            if header_lines > 0 {
                constraints.push(Constraint::Length(header_lines));
            }
            constraints.push(Constraint::Min(0));
            constraints.push(Constraint::Length(3));

            Layout::default()
                .direction(Direction::Vertical)
                .constraints(constraints)
                .split(area)
        };

        let mut chunk_idx = 0;

        if has_title {
            self.render_title(f, chunks[chunk_idx]);
            chunk_idx += 1;
        }

        if is_empty {
            chunk_idx += 1; // skip spacer
            self.render_centered_input(f, chunks[chunk_idx]);
        } else {
            self.render_messages(f, chunks[chunk_idx]);
            chunk_idx += 1;
            self.render_input(f, chunks[chunk_idx]);
        }
    }

    fn render_title(&self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let title = self.state.generated_title.as_deref().unwrap_or("New Chat");

        let title_widget = Paragraph::new(title)
            .style(
                Style::default()
                    .fg(theme.foreground)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(theme.border)),
            );

        f.render_widget(title_widget, area);
    }

    pub fn render_dropdowns(&mut self, f: &mut Frame) {
        self.state.dropdown_item_areas.clear();
        const VISIBLE_ITEMS: usize = 6;
        const SCROLLBAR_WIDTH: u16 = 1;

        if self.state.provider_dropdown_open {
            if let Some(anchor) = self.state.provider_hit_area {
                let total = self.providers.len();
                let max_visible = VISIBLE_ITEMS.min(total);

                let offset = self
                    .state
                    .dropdown_scroll_offset
                    .min(total.saturating_sub(max_visible));
                self.state.dropdown_scroll_offset = offset;

                let visible_providers: Vec<_> = self
                    .providers
                    .iter()
                    .skip(offset)
                    .take(max_visible)
                    .collect();

                let labels: Vec<String> = visible_providers
                    .iter()
                    .map(|(name, _, _, _, _)| clipped_label(name, 12))
                    .collect();
                let items: Vec<ListItem> = visible_providers
                    .iter()
                    .zip(labels.iter())
                    .map(|(provider, label)| {
                        let name = &provider.0;
                        let style = if name == &self.state.tab.provider {
                            Style::default().fg(Color::Black).bg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        ListItem::new(label.as_str()).style(style)
                    })
                    .collect();

                let content_height = max_visible as u16;
                let dropdown_width = dropdown_width_for(&labels, 12, SCROLLBAR_WIDTH);
                let dropdown_area = Rect::new(
                    anchor.x,
                    anchor.y.saturating_sub(content_height + 2),
                    dropdown_width,
                    content_height + 2,
                );
                let content_width = dropdown_area.width.saturating_sub(2 + SCROLLBAR_WIDTH);
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

                for i in 0..max_visible {
                    self.state.dropdown_item_areas.push(Rect {
                        x: viewport.x,
                        y: viewport.y + i as u16,
                        width: viewport.width,
                        height: 1,
                    });
                }
            }
        } else if self.state.model_dropdown_open {
            if let Some(anchor) = self.state.model_hit_area {
                let total = self.models.len();
                let max_visible = VISIBLE_ITEMS.min(total);

                let offset = self
                    .state
                    .dropdown_scroll_offset
                    .min(total.saturating_sub(max_visible));
                self.state.dropdown_scroll_offset = offset;

                let visible_models: Vec<_> =
                    self.models.iter().skip(offset).take(max_visible).collect();

                let labels: Vec<String> = visible_models
                    .iter()
                    .map(|model| clipped_label(&model.id, 16))
                    .collect();
                let items: Vec<ListItem> = visible_models
                    .iter()
                    .zip(labels.iter())
                    .map(|(m, label)| {
                        let is_selected = m.id == self.state.tab.model;
                        let style = if is_selected {
                            Style::default().fg(Color::Black).bg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        ListItem::new(label.as_str()).style(style)
                    })
                    .collect();

                let content_height = max_visible as u16;
                let dropdown_width = dropdown_width_for(&labels, 16, SCROLLBAR_WIDTH);
                let dropdown_area = Rect::new(
                    anchor.x,
                    anchor.y.saturating_sub(content_height + 2),
                    dropdown_width,
                    content_height + 2,
                );
                let content_width = dropdown_area.width.saturating_sub(2 + SCROLLBAR_WIDTH);
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

                for i in 0..max_visible {
                    self.state.dropdown_item_areas.push(Rect {
                        x: viewport.x,
                        y: viewport.y + i as u16,
                        width: viewport.width,
                        height: 1,
                    });
                }
            }
        } else if self.state.reasoning_dropdown_open {
            if let Some(anchor) = self.state.reasoning_hit_area {
                let total = self.reasoning_options.len();
                let max_visible = VISIBLE_ITEMS.min(total);

                let offset = self
                    .state
                    .dropdown_scroll_offset
                    .min(total.saturating_sub(max_visible));
                self.state.dropdown_scroll_offset = offset;

                let visible_options: Vec<_> = self
                    .reasoning_options
                    .iter()
                    .skip(offset)
                    .take(max_visible)
                    .collect();

                let labels: Vec<String> = visible_options
                    .iter()
                    .map(|option| clipped_label(option, 8))
                    .collect();
                let items: Vec<ListItem> = visible_options
                    .iter()
                    .zip(labels.iter())
                    .map(|(option, label)| {
                        let is_selected =
                            self.state.tab.reasoning_effort.as_deref() == Some(option.as_str());
                        let style = if is_selected {
                            Style::default().fg(Color::Black).bg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        ListItem::new(label.as_str()).style(style)
                    })
                    .collect();

                let content_height = max_visible as u16;
                let dropdown_width = dropdown_width_for(&labels, 8, 0);
                let dropdown_area = Rect::new(
                    anchor.x,
                    anchor.y.saturating_sub(content_height + 2),
                    dropdown_width,
                    content_height + 2,
                );
                let content_width = dropdown_area.width.saturating_sub(2);
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

                for i in 0..max_visible {
                    self.state.dropdown_item_areas.push(Rect {
                        x: viewport.x,
                        y: viewport.y + i as u16,
                        width: viewport.width,
                        height: 1,
                    });
                }
            }
        }
    }

    fn render_messages(&mut self, f: &mut Frame, area: Rect) {
        self.state.kitty_text_overlays.clear();
        if area.height == 0 {
            self.state.thinking_hit_areas.clear();
            self.state.link_hit_areas.clear();
            self.state.chat_scrollbar_area = None;
            self.state.chat_scrollbar_thumb = None;
            return;
        }
        let rendered = self.build_messages(area);
        let show_scrollbar = self.show_chat_scrollbar
            && rendered.lines.len() > area.height as usize
            && area.width > 1;
        let viewport = if show_scrollbar {
            Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height)
        } else {
            area
        };
        let total_lines = rendered.lines.len();
        let max_scroll = total_lines.saturating_sub(viewport.height as usize);
        let mut scroll_offset = self.state.scroll_offset.min(max_scroll);

        if self.state.streaming {
            scroll_offset = max_scroll;
        } else if let Some(message_idx) = self.state.scroll_to_message.take() {
            let anchor = rendered
                .answer_anchor_lines
                .iter()
                .find_map(|(idx, line)| (*idx == message_idx).then_some(*line))
                .unwrap_or(max_scroll);
            scroll_offset = anchor.min(max_scroll);
        }

        self.state.scroll_offset = scroll_offset;
        self.state.answer_anchor_lines = rendered.answer_anchor_lines.clone();
        self.state.total_rendered_lines = total_lines;
        self.state.message_viewport_height = viewport.height as usize;
        self.state.chat_scrollbar_area = None;
        self.state.chat_scrollbar_thumb = None;
        self.state.thinking_hit_areas.clear();
        self.state.link_hit_areas.clear();
        for (message_idx, line_idx) in rendered.thinking_toggle_lines {
            if line_idx >= scroll_offset && line_idx < scroll_offset + viewport.height as usize {
                self.state.thinking_hit_areas.push((
                    message_idx,
                    Rect::new(
                        viewport.x,
                        viewport.y + (line_idx - scroll_offset) as u16,
                        viewport.width,
                        1,
                    ),
                ));
            }
        }
        for target in &rendered.link_targets {
            if target.line >= scroll_offset
                && target.line < scroll_offset + viewport.height as usize
            {
                let line_x = rendered
                    .lines
                    .get(target.line)
                    .map(|line| aligned_line_x(viewport, line))
                    .unwrap_or(viewport.x);
                self.state.link_hit_areas.push((
                    Rect::new(
                        line_x.saturating_add(target.column as u16),
                        viewport.y + (target.line - scroll_offset) as u16,
                        target.width.min(u16::MAX as usize) as u16,
                        1,
                    ),
                    target.target.clone(),
                ));
            }
        }

        let list = Paragraph::new(rendered.lines)
            .block(Block::default().borders(Borders::NONE))
            .scroll((scroll_offset as u16, 0));

        f.render_widget(list, viewport);
        for heading in &rendered.kitty_headings {
            if heading.line < scroll_offset
                || heading.line >= scroll_offset + viewport.height as usize
            {
                continue;
            }
            let width =
                UnicodeWidthStr::width(heading.text.as_str()) as u16 * u16::from(heading.scale);
            let x = viewport.x
                + match heading.alignment {
                    Alignment::Left => 0,
                    Alignment::Center => viewport.width.saturating_sub(width) / 2,
                    Alignment::Right => viewport.width.saturating_sub(width),
                };
            let y = viewport.y + (heading.line - scroll_offset) as u16;
            self.state.kitty_text_overlays.push(KittyTextOverlay {
                x,
                y,
                text: heading.text.clone(),
                scale: heading.scale,
            });
        }
        self.render_images(f, viewport, scroll_offset, &rendered.images);
        if show_scrollbar {
            let track = Rect::new(viewport.right(), viewport.y, 1, viewport.height);
            let thumb =
                scrollbar_thumb(track, total_lines, viewport.height as usize, scroll_offset);
            self.state.chat_scrollbar_area = Some(track);
            self.state.chat_scrollbar_thumb = Some(thumb);
            f.render_widget(
                Paragraph::new(vec![Line::from("│"); track.height as usize])
                    .style(Style::default().fg(Color::DarkGray)),
                track,
            );
            f.render_widget(
                Paragraph::new(vec![Line::from("█"); thumb.height as usize])
                    .style(Style::default().fg(Color::White)),
                thumb,
            );
        }
    }

    fn build_messages(&self, area: Rect) -> RenderedMessages {
        let markdown = MarkdownRenderer::new(self.terminal_capabilities);
        let mut lines: Vec<Line> = Vec::new();
        let mut thinking_toggle_lines = Vec::new();
        let mut link_targets = Vec::new();
        let mut images = Vec::new();
        let mut kitty_headings = Vec::new();
        let mut answer_anchor_lines = Vec::new();
        let content_width = area.width.saturating_sub(2) as usize;

        for (idx, m) in self.state.messages.iter().enumerate() {
            let is_user = m.role == "user";
            let is_assistant = m.role == "assistant";
            let is_last_streaming = idx == self.state.messages.len() - 1 && self.state.streaming;
            let alignment = if is_user {
                self.user_alignment
            } else if is_assistant {
                self.ai_alignment
            } else {
                TextAlignment::Left
            };

            let (role_label, role_color) = match m.role.as_str() {
                "user" => ("You", Color::Green),
                "assistant" => ("Assistant", Color::Cyan),
                "system" => ("System", Color::Yellow),
                _ => ("", Color::White),
            };

            if !role_label.is_empty() {
                lines.push(Line::from(vec![Span::styled(
                    format!("{} ", role_label),
                    Style::default().fg(role_color).add_modifier(Modifier::BOLD),
                )]));
            }

            #[cfg(feature = "memory")]
            let (memory_before, memory_after) =
                memory_activity_lines(m, is_last_streaming, self.frame_tick);
            #[cfg(feature = "memory")]
            lines.extend(memory_before);

            let mut answer_anchor = lines.len();
            if is_assistant {
                if let Some(thinking) = m
                    .thinking_content
                    .as_deref()
                    .filter(|text| !text.is_empty())
                {
                    let collapsed = self.thinking_collapsed(idx);
                    let dots = if collapsed && is_last_streaming {
                        animated_dots(self.frame_tick)
                    } else {
                        ""
                    };
                    let toggle_label = if collapsed {
                        if is_last_streaming {
                            format!("> thinking{dots}")
                        } else {
                            "> show thinking".to_string()
                        }
                    } else {
                        "v hide thinking".to_string()
                    };
                    let toggle_line = lines.len();
                    lines.push(Line::from(Span::styled(
                        toggle_label,
                        Style::default().fg(Color::DarkGray),
                    )));
                    thinking_toggle_lines.push((idx, toggle_line));

                    if !collapsed {
                        let answer_context_lines = 5usize;
                        let rendered = markdown.render(
                            thinking,
                            self.markdown_mode,
                            content_width,
                            false,
                            self.kitty_text_max_scale,
                            false,
                        );
                        for mut target in rendered.link_targets {
                            target.line += lines.len();
                            link_targets.push(target);
                        }
                        for mut line in rendered.lines {
                            line.alignment = Some(alignment.as_ratatui());
                            line.style = Style::default().fg(Color::DarkGray);
                            lines.push(line);
                        }
                        answer_anchor = lines
                            .len()
                            .saturating_sub(answer_context_lines)
                            .max(toggle_line);
                    } else {
                        answer_anchor = toggle_line;
                    }
                    answer_anchor = if lines.len() > toggle_line + 1 {
                        answer_anchor
                    } else {
                        toggle_line
                    };
                }
            }

            let rendered = markdown.render(
                &m.content,
                self.markdown_mode,
                content_width,
                self.kitty_enhanced_text,
                self.kitty_text_max_scale,
                self.image_protocol != "off",
            );
            for heading in rendered.kitty_headings {
                kitty_headings.push(RenderedKittyHeading {
                    line: heading.start_line + lines.len(),
                    text: heading.text,
                    scale: heading.scale,
                    alignment: alignment.as_ratatui(),
                });
            }
            if !rendered.lines.is_empty() {
                answer_anchor = answer_anchor.min(lines.len());
            }
            for image in rendered.images {
                images.push(RenderedImage {
                    start_line: image.start_line + lines.len(),
                    height: image.height,
                    source: image.source,
                });
            }
            for mut target in rendered.link_targets {
                target.line += lines.len();
                link_targets.push(target);
            }
            for mut line in rendered.lines {
                line.alignment = Some(alignment.as_ratatui());
                lines.push(line);
            }
            #[cfg(feature = "memory")]
            lines.extend(memory_after);
            answer_anchor_lines.push((idx, answer_anchor));

            if idx < self.state.messages.len() - 1 || !is_last_streaming {
                lines.push(Line::from(""));
            }
        }

        RenderedMessages {
            lines,
            thinking_toggle_lines,
            link_targets,
            images,
            kitty_headings,
            answer_anchor_lines,
        }
    }

    fn render_images(
        &mut self,
        f: &mut Frame,
        area: Rect,
        scroll_offset: usize,
        images: &[RenderedImage],
    ) {
        for image in images {
            if image.start_line + image.height <= scroll_offset
                || image.start_line >= scroll_offset + area.height as usize
            {
                continue;
            }
            if !is_local_image_source(&image.source) {
                continue;
            }
            let image_area = Rect::new(
                area.x,
                area.y + image.start_line.saturating_sub(scroll_offset) as u16,
                area.width,
                image.height.min(area.height as usize) as u16,
            );
            if !self.state.image_states.contains_key(&image.source) {
                if let Some(state) = ImageBlockState::from_source(
                    &image.source,
                    self.image_protocol,
                    self.terminal_capabilities,
                ) {
                    self.state.image_states.insert(image.source.clone(), state);
                } else {
                    continue;
                }
            }
            let Some(state) = self.state.image_states.get_mut(&image.source) else {
                continue;
            };
            state.render(f, image_area);
        }
    }

    fn thinking_collapsed(&self, message_idx: usize) -> bool {
        if self.state.thinking_fold_overrides.contains(&message_idx) {
            !self.collapse_thinking
        } else {
            self.collapse_thinking
        }
    }

    fn render_input(&mut self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        self.state.input_area = Some(area);
        let inner = Rect::new(
            area.x.saturating_add(1),
            area.y.saturating_add(1),
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );
        self.state.input_text_area = Some(inner);
        let (display, scroll, cursor_x) = visible_input_line(
            &self.state.input_content,
            self.state.input_cursor,
            self.state.input_scroll,
            inner.width.saturating_sub(1) as usize,
            true,
        );
        self.state.input_scroll = scroll;

        let input = Paragraph::new(format!(" {display}"))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border))
                    .title(" Message ")
                    .title_style(Style::default().fg(theme.muted)),
            )
            .style(if self.state.input_content.is_empty() {
                Style::default().fg(theme.muted).bg(theme.panel)
            } else {
                Style::default().fg(theme.foreground).bg(theme.panel)
            });

        f.render_widget(input, area);
        if inner.width > 0 && inner.height > 0 {
            f.set_cursor_position((inner.x.saturating_add(cursor_x).saturating_add(1), inner.y));
        }
    }

    fn render_centered_input(&mut self, f: &mut Frame, area: Rect) {
        let theme = crate::theme::active_theme();
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(12),
                Constraint::Percentage(76),
                Constraint::Percentage(12),
            ])
            .split(area);

        self.state.input_area = Some(horizontal[1]);
        let inner = Rect::new(
            horizontal[1].x.saturating_add(1),
            horizontal[1].y.saturating_add(1),
            horizontal[1].width.saturating_sub(2),
            horizontal[1].height.saturating_sub(2),
        );
        self.state.input_text_area = Some(inner);
        let (display, scroll, cursor_x) = visible_input_line(
            &self.state.input_content,
            self.state.input_cursor,
            self.state.input_scroll,
            inner.width.saturating_sub(1) as usize,
            true,
        );
        self.state.input_scroll = scroll;

        let input = Paragraph::new(format!(" {display}"))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.accent))
                    .title(" Start a conversation ")
                    .title_style(
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
            )
            .style(if self.state.input_content.is_empty() {
                Style::default().fg(theme.muted).bg(theme.panel)
            } else {
                Style::default().fg(theme.foreground).bg(theme.panel)
            });

        f.render_widget(input, horizontal[1]);
        if inner.width > 0 && inner.height > 0 {
            f.set_cursor_position((inner.x.saturating_add(cursor_x).saturating_add(1), inner.y));
        }
    }
}

fn visible_input_line(
    content: &str,
    cursor: usize,
    scroll: usize,
    width: usize,
    show_placeholder: bool,
) -> (String, usize, u16) {
    if width == 0 {
        return (String::new(), scroll, 0);
    }

    if content.is_empty() {
        let placeholder = if show_placeholder {
            "Type your message... (/quit to exit)"
        } else {
            ""
        };
        let visible: String = placeholder.chars().take(width).collect();
        return (visible, 0, 0);
    }

    let chars: Vec<char> = content.chars().collect();
    let available = width;
    let cursor = cursor.min(chars.len());
    let mut scroll = scroll.min(chars.len());
    if cursor < scroll {
        scroll = cursor;
    }
    if cursor > scroll + available {
        scroll = cursor.saturating_sub(available);
    }
    let end = (scroll + available).min(chars.len());
    let visible: String = chars[scroll..end].iter().collect();
    let relative = cursor.saturating_sub(scroll).min(available) as u16;
    (visible, scroll, relative)
}

fn scrollbar_thumb(
    track: Rect,
    total_lines: usize,
    viewport_height: usize,
    scroll_offset: usize,
) -> Rect {
    let thumb_height = ((viewport_height as f64 / total_lines as f64) * track.height as f64)
        .round()
        .max(1.0) as u16;
    let max_scroll = total_lines.saturating_sub(viewport_height).max(1);
    let thumb_y = ((scroll_offset as f64 / max_scroll as f64)
        * track.height.saturating_sub(thumb_height) as f64)
        .round() as u16;
    Rect::new(
        track.x,
        track.y + thumb_y,
        track.width,
        thumb_height.min(track.height),
    )
}

fn animated_dots(frame_tick: u64) -> &'static str {
    match (frame_tick / 10) % 4 {
        0 => ".",
        1 => "..",
        2 => "...",
        _ => "",
    }
}

fn clipped_label(label: &str, max_chars: usize) -> String {
    if label.chars().count() <= max_chars {
        return label.to_string();
    }
    let mut clipped = label
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    clipped.push('~');
    clipped
}

fn dropdown_width_for(labels: &[String], max_chars: usize, scrollbar_width: u16) -> u16 {
    let content_width = labels
        .iter()
        .map(|label| UnicodeWidthStr::width(label.as_str()))
        .max()
        .unwrap_or(1)
        .min(max_chars)
        .max(1) as u16;
    content_width + 2 + scrollbar_width
}

#[cfg(feature = "memory")]
fn memory_activity_lines(
    message: &crate::app::Message,
    is_streaming: bool,
    frame_tick: u64,
) -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    let theme = crate::theme::active_theme();
    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut saving = false;
    for activity in crate::memory::activities(message) {
        let (label, color, below_answer) = match activity {
            crate::memory::MemoryActivity::Recalling => (
                format!(
                    "> recalling memory{}",
                    if is_streaming {
                        animated_dots(frame_tick)
                    } else {
                        ""
                    }
                ),
                Color::DarkGray,
                false,
            ),
            crate::memory::MemoryActivity::Recalled { titles } => (
                format!(
                    "> recalled {} {}",
                    titles.len(),
                    if titles.len() == 1 {
                        "memory"
                    } else {
                        "memories"
                    }
                ),
                Color::DarkGray,
                false,
            ),
            crate::memory::MemoryActivity::Saving => {
                saving = true;
                (
                    format!(
                        "> saving memory{}",
                        if is_streaming {
                            animated_dots(frame_tick)
                        } else {
                            ""
                        }
                    ),
                    Color::DarkGray,
                    true,
                )
            }
            crate::memory::MemoryActivity::Saved { title, .. } => {
                saving = true;
                (format!("> saved memory: {title}"), Color::DarkGray, true)
            }
            crate::memory::MemoryActivity::AlreadyKnown { title } => {
                saving = true;
                (
                    format!("> memory already known: {title}"),
                    Color::DarkGray,
                    true,
                )
            }
            crate::memory::MemoryActivity::Failed { .. } => {
                ("> memory unavailable".to_string(), theme.error, saving)
            }
        };
        let line = Line::from(Span::styled(label, Style::default().fg(color)));
        if below_answer {
            after.push(line);
        } else {
            before.push(line);
        }
    }
    (before, after)
}

fn aligned_line_x(area: Rect, line: &Line<'_>) -> u16 {
    let remaining = area.width.saturating_sub(line.width() as u16);
    area.x
        + match line.alignment.unwrap_or(Alignment::Left) {
            Alignment::Left => 0,
            Alignment::Center => remaining / 2,
            Alignment::Right => remaining,
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::components::terminal_capabilities::{TerminalCapabilities, TerminalKind};
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn skill_mentions_on_one_line_have_independent_hit_areas() {
        // Given
        let mut ui = crate::ui::UI::new();
        ui.tabs[0].messages.push(crate::app::message::Message::new(
            1,
            "assistant".to_string(),
            "Use @caveman then @save.".to_string(),
        ));
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        let mut chat = ChatTab::new(
            &mut ui.tabs[0],
            ChatTabProps {
                user_alignment: TextAlignment::Left,
                ai_alignment: TextAlignment::Right,
                markdown_mode: MarkdownMode::Full,
                collapse_thinking: true,
                show_chat_scrollbar: true,
                kitty_enhanced_text: false,
                kitty_text_max_scale: 3,
                image_protocol: "off",
                terminal_capabilities: TerminalCapabilities {
                    terminal: TerminalKind::Unknown,
                    multiplexer: None,
                    kitty_graphics: false,
                    kitty_text_sizing: false,
                    tmux_passthrough: false,
                },
                frame_tick: 0,
                providers: &[],
                models: &[],
                reasoning_options: &[],
            },
        );

        // When
        terminal
            .draw(|frame| chat.render_messages(frame, Rect::new(0, 0, 80, 10)))
            .expect("render messages");

        // Then
        assert_eq!(chat.state.link_hit_areas.len(), 2);
        assert_eq!(chat.state.link_hit_areas[0].1, "skill:caveman");
        assert_eq!(chat.state.link_hit_areas[1].1, "skill:save");
        assert_eq!(chat.state.link_hit_areas[0].0.width, 8);
        assert_eq!(chat.state.link_hit_areas[1].0.width, 5);
        assert_eq!(chat.state.link_hit_areas[0].0.x, 60);
        assert_eq!(chat.state.link_hit_areas[1].0.x, 74);
        assert!(
            chat.state.link_hit_areas[0].0.x + chat.state.link_hit_areas[0].0.width
                <= chat.state.link_hit_areas[1].0.x
        );
    }

    #[test]
    fn visible_input_line_scrolls_to_keep_cursor_visible() {
        let (display, scroll, cursor_x) = visible_input_line("abcdefghij", 10, 0, 5, false);

        assert_eq!(scroll, 5);
        assert_eq!(cursor_x, 5);
        assert_eq!(display, "fghij");
    }

    #[test]
    fn unfolded_thinking_anchor_keeps_context_above_answer() {
        let mut ui = crate::ui::UI::new();
        let mut message =
            crate::app::message::Message::new(1, "assistant".to_string(), "Answer".to_string());
        message.thinking_content = Some(
            [
                "one", "two", "three", "four", "five", "six", "seven", "eight",
            ]
            .join("\n"),
        );
        ui.tabs[0].messages.push(message);
        ui.tabs[0].thinking_fold_overrides.insert(0);
        let chat = ChatTab::new(
            &mut ui.tabs[0],
            ChatTabProps {
                user_alignment: TextAlignment::Left,
                ai_alignment: TextAlignment::Left,
                markdown_mode: MarkdownMode::Off,
                collapse_thinking: true,
                show_chat_scrollbar: true,
                kitty_enhanced_text: false,
                kitty_text_max_scale: 3,
                image_protocol: "off",
                terminal_capabilities: TerminalCapabilities {
                    terminal: TerminalKind::Unknown,
                    multiplexer: None,
                    kitty_graphics: false,
                    kitty_text_sizing: false,
                    tmux_passthrough: false,
                },
                frame_tick: 0,
                providers: &[],
                models: &[],
                reasoning_options: &[],
            },
        );

        let rendered = chat.build_messages(Rect::new(0, 0, 40, 12));
        let anchor = rendered.answer_anchor_lines[0].1;

        assert_eq!(rendered.lines[anchor].to_string().trim(), "four");
    }

    #[test]
    fn bottom_scroll_keeps_long_markdown_tail_and_thinking_hit_area_aligned() {
        // Given
        let mut ui = crate::ui::UI::new();
        ui.tabs[0].messages.push(crate::app::message::Message::new(
            1,
            "user".to_string(),
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ".to_string(),
        ));
        let mut assistant =
            crate::app::message::Message::new(1, "assistant".to_string(), "final tail".to_string());
        assistant.thinking_content = Some("reason".to_string());
        ui.tabs[0].messages.push(assistant);
        ui.tabs[0].scroll_offset = usize::MAX;
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).expect("test terminal");
        let mut chat = ChatTab::new(
            &mut ui.tabs[0],
            ChatTabProps {
                user_alignment: TextAlignment::Left,
                ai_alignment: TextAlignment::Left,
                markdown_mode: MarkdownMode::Full,
                collapse_thinking: true,
                show_chat_scrollbar: false,
                kitty_enhanced_text: false,
                kitty_text_max_scale: 3,
                image_protocol: "off",
                terminal_capabilities: TerminalCapabilities {
                    terminal: TerminalKind::Unknown,
                    multiplexer: None,
                    kitty_graphics: false,
                    kitty_text_sizing: false,
                    tmux_passthrough: false,
                },
                frame_tick: 0,
                providers: &[],
                models: &[],
                reasoning_options: &[],
            },
        );

        // When
        terminal
            .draw(|frame| chat.render_messages(frame, Rect::new(0, 0, 20, 4)))
            .expect("render messages");
        let screen = terminal.backend().to_string();

        // Then
        assert!(screen.contains("final tail"), "{screen}");
        let toggle_row = screen
            .lines()
            .position(|line| line.contains("> show thinking"))
            .expect("visible thinking toggle") as u16;
        assert_eq!(chat.state.thinking_hit_areas[0].1.y, toggle_row);
    }

    #[test]
    fn kitty_heading_is_not_embedded_in_ratatui_cells() {
        // Given
        let mut ui = crate::ui::UI::new();
        ui.tabs[0].messages.push(crate::app::message::Message::new(
            1,
            "assistant".to_string(),
            "# Heading".to_string(),
        ));
        let mut terminal = Terminal::new(TestBackend::new(40, 6)).expect("test terminal");
        let mut chat = ChatTab::new(
            &mut ui.tabs[0],
            ChatTabProps {
                user_alignment: TextAlignment::Left,
                ai_alignment: TextAlignment::Left,
                markdown_mode: MarkdownMode::Full,
                collapse_thinking: true,
                show_chat_scrollbar: false,
                kitty_enhanced_text: true,
                kitty_text_max_scale: 2,
                image_protocol: "off",
                terminal_capabilities: TerminalCapabilities {
                    terminal: TerminalKind::Kitty,
                    multiplexer: None,
                    kitty_graphics: true,
                    kitty_text_sizing: true,
                    tmux_passthrough: false,
                },
                frame_tick: 0,
                providers: &[],
                models: &[],
                reasoning_options: &[],
            },
        );

        // When
        terminal
            .draw(|frame| chat.render_messages(frame, Rect::new(0, 0, 40, 6)))
            .expect("render kitty heading");

        // Then
        assert!(!terminal
            .backend()
            .buffer()
            .content
            .iter()
            .any(|cell| cell.symbol().contains("\u{1b}]66;")));
        assert_eq!(chat.state.kitty_text_overlays.len(), 1);
    }

    #[test]
    fn render_capture_shows_scrollbar_and_input_cursor() {
        let mut ui = crate::ui::UI::new();
        for idx in 0..12 {
            ui.tabs[0].messages.push(crate::app::message::Message::new(
                1,
                if idx % 2 == 0 {
                    "user".to_string()
                } else {
                    "assistant".to_string()
                },
                format!("Message number {idx}"),
            ));
        }
        ui.tabs[0].input_content = "hello wide viewport".to_string();
        ui.tabs[0].input_cursor = 5;
        ui.tabs[0].scroll_offset = 4;

        let mut terminal = Terminal::new(TestBackend::new(80, 18)).expect("test terminal");
        let mut chat = ChatTab::new(
            &mut ui.tabs[0],
            ChatTabProps {
                user_alignment: TextAlignment::Left,
                ai_alignment: TextAlignment::Left,
                markdown_mode: MarkdownMode::Off,
                collapse_thinking: true,
                show_chat_scrollbar: true,
                kitty_enhanced_text: false,
                kitty_text_max_scale: 3,
                image_protocol: "off",
                terminal_capabilities: TerminalCapabilities {
                    terminal: TerminalKind::Unknown,
                    multiplexer: None,
                    kitty_graphics: false,
                    kitty_text_sizing: false,
                    tmux_passthrough: false,
                },
                frame_tick: 0,
                providers: &[],
                models: &[],
                reasoning_options: &[],
            },
        );

        terminal
            .draw(|frame| chat.render(frame, Rect::new(0, 0, 80, 18)))
            .expect("render chat");

        if let Some(path) = std::env::var_os("TCUI_SCROLL_CAPTURE") {
            let buffer = terminal.backend().buffer();
            let mut capture = String::new();
            for y in 0..buffer.area.height {
                let mut skipped = 0usize;
                for x in 0..buffer.area.width {
                    if skipped > 0 {
                        skipped -= 1;
                        continue;
                    }
                    let symbol = buffer[(x, y)].symbol();
                    capture.push_str(symbol);
                    skipped = unicode_width::UnicodeWidthStr::width(symbol).saturating_sub(1);
                }
                if y + 1 < buffer.area.height {
                    capture.push('\n');
                }
            }
            std::fs::write(path, capture).expect("write scroll capture");
        }

        assert!(chat.state.chat_scrollbar_area.is_some());
        assert!(chat.state.input_text_area.is_some());
    }

    #[test]
    fn dropdown_width_caps_long_provider_and_model_labels() {
        let provider_labels = vec![clipped_label("VeryLongProviderName", 12)];
        let model_labels = vec![clipped_label("deepseek-v4-flash", 16)];

        assert_eq!(provider_labels[0], "VeryLongPro~");
        assert_eq!(model_labels[0], "deepseek-v4-fla~");
        assert_eq!(dropdown_width_for(&provider_labels, 12, 1), 15);
        assert_eq!(dropdown_width_for(&model_labels, 16, 1), 19);
    }

    #[cfg(feature = "memory")]
    #[test]
    fn memory_activity_surrounds_thinking_and_answer() {
        // Given
        let mut ui = crate::ui::UI::new();
        let mut message =
            crate::app::message::Message::new(1, "assistant".to_string(), "Answer".to_string());
        message.thinking_content = Some("Reason".to_string());
        crate::memory::set_activities(
            &mut message,
            &[
                crate::memory::MemoryActivity::Recalled {
                    titles: vec!["Preferred editor".to_string()],
                },
                crate::memory::MemoryActivity::Saved {
                    title: "Concise answers".to_string(),
                    path: "concise-answers.md".to_string(),
                },
                crate::memory::MemoryActivity::AlreadyKnown {
                    title: "好みのエディター".to_string(),
                },
            ],
        )
        .expect("memory activity");
        ui.tabs[0].messages.push(message);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        let mut chat = ChatTab::new(
            &mut ui.tabs[0],
            ChatTabProps {
                user_alignment: TextAlignment::Left,
                ai_alignment: TextAlignment::Left,
                markdown_mode: MarkdownMode::Off,
                collapse_thinking: true,
                show_chat_scrollbar: true,
                kitty_enhanced_text: false,
                kitty_text_max_scale: 3,
                image_protocol: "off",
                terminal_capabilities: TerminalCapabilities {
                    terminal: TerminalKind::Unknown,
                    multiplexer: None,
                    kitty_graphics: false,
                    kitty_text_sizing: false,
                    tmux_passthrough: false,
                },
                frame_tick: 0,
                providers: &[],
                models: &[],
                reasoning_options: &[],
            },
        );

        // When
        terminal
            .draw(|frame| chat.render_messages(frame, Rect::new(0, 0, 80, 12)))
            .expect("render messages");
        let screen = terminal.backend().to_string();
        if let Some(path) = std::env::var_os("TCUI_MEMORY_CAPTURE") {
            let buffer = terminal.backend().buffer();
            let mut capture = String::new();
            for y in 0..buffer.area.height {
                let mut skipped = 0usize;
                for x in 0..buffer.area.width {
                    if skipped > 0 {
                        skipped -= 1;
                        continue;
                    }
                    let symbol = buffer[(x, y)].symbol();
                    capture.push_str(symbol);
                    skipped = unicode_width::UnicodeWidthStr::width(symbol).saturating_sub(1);
                }
                if y + 1 < buffer.area.height {
                    capture.push('\n');
                }
            }
            std::fs::write(path, capture).expect("write memory activity capture");
        }

        // Then
        let recalled = screen
            .find("> recalled 1 memory")
            .expect("recalled activity");
        let thinking = screen.find("> show thinking").expect("thinking activity");
        let answer = screen.find("Answer").expect("answer");
        let saved = screen
            .find("> saved memory: Concise answers")
            .expect("saved activity");
        assert!(recalled < thinking && thinking < answer && answer < saved);
        assert!(screen.contains("> memory already known: 好みのエディター"));
    }

    #[cfg(feature = "memory")]
    #[test]
    fn memory_activity_renders_active_duplicate_and_failure_states() {
        // Given
        let mut message =
            crate::app::Message::new(1, "assistant".to_string(), "Answer".to_string());
        crate::memory::set_activities(
            &mut message,
            &[
                crate::memory::MemoryActivity::Recalling,
                crate::memory::MemoryActivity::Saving,
                crate::memory::MemoryActivity::AlreadyKnown {
                    title: "Editor".to_string(),
                },
                crate::memory::MemoryActivity::Failed {
                    message: "database unavailable".to_string(),
                },
            ],
        )
        .expect("memory activity");

        // When
        let (before, after) = memory_activity_lines(&message, true, 20);
        let before = before
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        let after = after
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        // Then
        assert!(before.contains("> recalling memory..."));
        assert!(after.contains("> saving memory..."));
        assert!(after.contains("> memory already known: Editor"));
        assert!(after.contains("> memory unavailable"));
        assert!(!after.contains("database unavailable"));
    }
}
