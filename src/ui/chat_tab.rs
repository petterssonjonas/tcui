#![allow(dead_code)]
use crate::config::app_config::{HeadingDownscale, MarkdownMode, TextAlignment};
use crate::ui::ModelInfo;
use crate::ui::components::image_block::{ImageBlockState, is_local_image_source};
use crate::ui::components::markdown::MarkdownRenderer;
use crate::ui::components::markdown_model::{KittyHeadingTier, LinkTarget, RenderedImage};
use ratatui::{
    Frame,
    layout::{Rect, Size},
    prelude::*,
    widgets::*,
};
use ratatui_image::sliced::SignedPosition;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const INPUT_MARGIN_HORIZONTAL: u16 = 2;
const INPUT_MARGIN_VERTICAL: u16 = 1;

pub struct ChatTab<'a> {
    pub state: &'a mut crate::ui::ChatTabState,
    pub user_alignment: TextAlignment,
    pub ai_alignment: TextAlignment,
    pub markdown_mode: MarkdownMode,
    pub collapse_thinking: bool,
    pub show_chat_scrollbar: bool,
    pub kitty_enhanced_text: bool,
    pub kitty_heading_downscale: HeadingDownscale,
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
    pub kitty_heading_downscale: HeadingDownscale,
    pub image_protocol: &'a str,
    pub terminal_capabilities: crate::ui::components::terminal_capabilities::TerminalCapabilities,
    pub frame_tick: u64,
    pub providers: &'a [(String, String, String, String, String)],
    pub models: &'a [ModelInfo],
    pub reasoning_options: &'a [String],
}

struct RenderedMessages {
    lines: Vec<Line<'static>>,
    thinking_toggle_lines: Vec<ThinkingToggleLine>,
    link_targets: Vec<LinkTarget>,
    images: Vec<RenderedImage>,
    kitty_headings: Vec<RenderedKittyHeading>,
    answer_anchor_lines: Vec<(usize, usize)>,
}

struct ThinkingToggleLine {
    message_idx: usize,
    line: usize,
    x_offset: u16,
    width: u16,
}

struct RenderedKittyHeading {
    line: usize,
    text: String,
    tier: KittyHeadingTier,
    style: Style,
    alignment: Alignment,
}

pub(crate) struct InputLayout {
    pub(crate) visible_lines: Vec<String>,
    pub(crate) line_ranges: Vec<(usize, usize)>,
    pub(crate) scroll: usize,
    pub(crate) cursor_x: u16,
    pub(crate) cursor_y: u16,
    pub(crate) total_lines: usize,
    pub(crate) show_scrollbar: bool,
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
            kitty_heading_downscale: props.kitty_heading_downscale,
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
        let input_height = if is_empty {
            centered_input_height(self.state, area)
        } else {
            bottom_input_height(self.state, area)
        };

        let chunks = if is_empty {
            let mut constraints = vec![];
            if header_lines > 0 {
                constraints.push(Constraint::Length(header_lines));
            }
            constraints.push(Constraint::Min(0));
            constraints.push(Constraint::Length(input_height));
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
            constraints.push(Constraint::Length(input_height));

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
            self.render_messages(f, chat_messages_area(chunks[chunk_idx]));
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
            .block(Block::default().style(Style::default().bg(theme.panel)));

        f.render_widget(title_widget, area);
    }

    pub fn render_dropdowns(&mut self, f: &mut Frame) {
        self.state.dropdown_item_areas.clear();
        const VISIBLE_ITEMS: usize = 6;
        const PROVIDER_DROPDOWN_MIN_WIDTH: usize = 30;
        const MODEL_DROPDOWN_MIN_WIDTH: usize = 30;
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
                    .map(|(name, _, _, _, _)| name.clone())
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
                let dropdown_width =
                    dropdown_width_for(&labels, PROVIDER_DROPDOWN_MIN_WIDTH, SCROLLBAR_WIDTH);
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
                    Block::default().style(Style::default().bg(Color::Black)),
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
                    .map(|model| model.id.clone())
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
                let dropdown_width =
                    dropdown_width_for(&labels, MODEL_DROPDOWN_MIN_WIDTH, SCROLLBAR_WIDTH);
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
                    Block::default().style(Style::default().bg(Color::Black)),
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
                    Block::default().style(Style::default().bg(Color::Black)),
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
        for toggle in rendered.thinking_toggle_lines {
            let line_idx = toggle.line;
            if line_idx >= scroll_offset && line_idx < scroll_offset + viewport.height as usize {
                self.state.thinking_hit_areas.push((
                    toggle.message_idx,
                    Rect::new(
                        viewport.x.saturating_add(toggle.x_offset),
                        viewport.y + (line_idx - scroll_offset) as u16,
                        toggle
                            .width
                            .min(viewport.width.saturating_sub(toggle.x_offset)),
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
        self.render_kitty_headings(f, viewport, scroll_offset, &rendered.kitty_headings);
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
        let theme = crate::theme::active_theme();
        let markdown = MarkdownRenderer::new(self.terminal_capabilities);
        let mut lines: Vec<Line> = Vec::new();
        let mut thinking_toggle_lines = Vec::new();
        let mut link_targets = Vec::new();
        let mut images = Vec::new();
        let mut kitty_headings = Vec::new();
        let mut answer_anchor_lines = Vec::new();
        let content_width = area.width.saturating_sub(2) as usize;
        let assistant_box_width = area.width.max(2);
        let assistant_inner_width = assistant_box_width.saturating_sub(2) as usize;
        let user_inner_width = assistant_box_width.saturating_sub(4) as usize;
        let thinking_x_offset = area.width / 10;
        let thinking_box_width = area.width.saturating_mul(8).max(10) / 10;
        let thinking_inner_width = thinking_box_width.saturating_sub(4) as usize;

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

            if m.role == "system" {
                lines.push(Line::from(vec![Span::styled(
                    " System ",
                    Style::default().fg(theme.warning),
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
                    let indicator = if collapsed { '▸' } else { '▾' };
                    let toggle_label = if collapsed && is_last_streaming {
                        format!("{indicator} thinking{dots}")
                    } else if collapsed {
                        format!("{indicator} show thinking")
                    } else {
                        format!("{indicator} hide thinking")
                    };
                    let toggle_line = lines.len();
                    lines.push(box_top_line(
                        thinking_x_offset as usize,
                        thinking_box_width as usize,
                        &toggle_label,
                        Style::default()
                            .fg(Color::Yellow)
                            .bg(theme.code_bg)
                            .add_modifier(Modifier::BOLD),
                        Style::default().bg(theme.code_bg),
                    ));
                    thinking_toggle_lines.push(ThinkingToggleLine {
                        message_idx: idx,
                        line: toggle_line,
                        x_offset: thinking_x_offset,
                        width: thinking_box_width,
                    });

                    if !collapsed {
                        let answer_context_lines = 6usize;
                        let rendered = markdown.render(
                            thinking,
                            self.markdown_mode,
                            thinking_inner_width,
                            false,
                            self.kitty_heading_downscale,
                            false,
                        );
                        let content_start = lines.len();
                        let content_pads = rendered
                            .lines
                            .iter()
                            .map(|line| {
                                alignment_padding_for(
                                    thinking_inner_width,
                                    line.width(),
                                    alignment.as_ratatui(),
                                )
                            })
                            .collect::<Vec<_>>();
                        for mut target in rendered.link_targets {
                            let pad = content_pads.get(target.line).copied().unwrap_or_default();
                            target.line += content_start;
                            target.column += thinking_x_offset as usize + 2 + pad;
                            link_targets.push(target);
                        }
                        for mut line in rendered.lines {
                            line.alignment = Some(alignment.as_ratatui());
                            line.style = line.style.fg(theme.muted).bg(theme.code_bg);
                            lines.push(box_content_line(
                                thinking_x_offset as usize,
                                thinking_inner_width,
                                line,
                                Style::default().bg(theme.code_bg),
                            ));
                        }
                        lines.push(box_bottom_line(
                            thinking_x_offset as usize,
                            thinking_box_width as usize,
                            Style::default().bg(theme.code_bg),
                        ));
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
                if is_user {
                    user_inner_width
                } else if is_assistant {
                    assistant_inner_width
                } else {
                    content_width
                },
                self.kitty_enhanced_text,
                self.kitty_heading_downscale,
                self.image_protocol != "off",
            );
            let user_name = if is_user {
                std::env::var("USER")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .or_else(|| std::env::var("USERNAME").ok())
                    .unwrap_or_else(|| "User".to_string())
            } else {
                String::new()
            };
            let assistant_content_start = if is_user {
                lines.push(box_top_line(
                    0,
                    assistant_box_width as usize,
                    &user_name,
                    Style::default()
                        .fg(Color::Cyan)
                        .bg(theme.user_bubble)
                        .add_modifier(Modifier::BOLD),
                    Style::default().bg(theme.user_bubble),
                ));
                lines.len()
            } else {
                lines.len()
            };
            let inner_w = if is_user {
                user_inner_width
            } else {
                assistant_inner_width
            };
            let content_pads = rendered
                .lines
                .iter()
                .map(|line| alignment_padding_for(inner_w, line.width(), alignment.as_ratatui()))
                .collect::<Vec<_>>();
            for heading in rendered.kitty_headings {
                kitty_headings.push(RenderedKittyHeading {
                    line: heading.start_line + assistant_content_start,
                    text: heading.text,
                    tier: heading.tier,
                    style: heading.style,
                    alignment: alignment.as_ratatui(),
                });
            }
            if !rendered.lines.is_empty() {
                answer_anchor = answer_anchor.min(assistant_content_start);
            }
            for image in rendered.images {
                images.push(RenderedImage {
                    start_line: image.start_line + assistant_content_start,
                    height: image.height,
                    source: image.source,
                });
            }
            for mut target in rendered.link_targets {
                if is_user {
                    let pad = content_pads.get(target.line).copied().unwrap_or_default();
                    target.column += 2 + pad;
                }
                target.line += assistant_content_start;
                link_targets.push(target);
            }
            for mut line in rendered.lines {
                line.alignment = Some(alignment.as_ratatui());
                if is_user {
                    line.style = line.style.bg(theme.user_bubble);
                    lines.push(box_content_line(
                        0,
                        user_inner_width,
                        line,
                        Style::default().bg(theme.user_bubble),
                    ));
                } else {
                    lines.push(line);
                }
            }
            if is_user {
                lines.push(box_bottom_line(
                    0,
                    assistant_box_width as usize,
                    Style::default().bg(theme.user_bubble),
                ));
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
            let relative_top = image.start_line as i16 - scroll_offset as i16;
            state.render_sliced(
                f,
                area,
                Size::new(area.width, image.height.min(u16::MAX as usize) as u16),
                SignedPosition::from((0, relative_top)),
            );
        }
    }

    fn render_kitty_headings(
        &self,
        f: &mut Frame,
        viewport: Rect,
        scroll_offset: usize,
        headings: &[RenderedKittyHeading],
    ) {
        for heading in headings {
            if heading.line < scroll_offset {
                continue;
            }
            let visible_row = heading.line - scroll_offset;
            if visible_row + 1 >= viewport.height as usize {
                continue;
            }
            let width = heading.tier.rendered_width(&heading.text);
            let Some(width) = u16::try_from(width)
                .ok()
                .filter(|width| *width <= viewport.width)
            else {
                continue;
            };
            let x = viewport.x
                + match heading.alignment {
                    Alignment::Left => 0,
                    Alignment::Center => viewport.width.saturating_sub(width) / 2,
                    Alignment::Right => viewport.width.saturating_sub(width),
                };
            crate::ui::components::terminal_capabilities::render_kitty_heading(
                f.buffer_mut(),
                Rect::new(x, viewport.y + visible_row as u16, width, 2),
                &heading.text,
                heading.tier,
                heading.style,
                self.terminal_capabilities,
            );
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
        let box_area = input_box_area(area);
        self.state.input_area = Some(box_area);
        let max_body_lines = box_area.height.saturating_sub(2) as usize;
        let layout = input_layout(
            &self.state.input_content,
            self.state.input_cursor,
            self.state.input_scroll,
            box_area.width.saturating_sub(2) as usize,
            max_body_lines,
            true,
        );
        self.state.input_scroll = layout.scroll;
        let inner_width = box_area
            .width
            .saturating_sub(2)
            .saturating_sub(u16::from(layout.show_scrollbar));
        let inner = Rect::new(
            box_area.x.saturating_add(1),
            box_area.y.saturating_add(1),
            inner_width,
            box_area.height.saturating_sub(2),
        );
        self.state.input_text_area = Some(inner);

        let input = Paragraph::new(
            layout
                .visible_lines
                .iter()
                .map(|line| Line::from(format!(" {line}")))
                .collect::<Vec<_>>(),
        )
        .block(
            Block::default()
                .style(Style::default().bg(theme.code_bg))
                .title(" Message ")
                .title_style(Style::default().fg(theme.muted)),
        )
        .style(if self.state.input_content.is_empty() {
            Style::default().fg(theme.muted).bg(theme.code_bg)
        } else {
            Style::default().fg(theme.foreground).bg(theme.code_bg)
        });

        f.render_widget(input, box_area);
        if layout.show_scrollbar {
            let track = Rect::new(box_area.right().saturating_sub(2), inner.y, 1, inner.height);
            let thumb = scrollbar_thumb(
                track,
                layout.total_lines,
                inner.height as usize,
                layout.scroll,
            );
            f.render_widget(
                Paragraph::new(vec![Line::from("│"); track.height as usize])
                    .style(Style::default().fg(theme.muted)),
                track,
            );
            f.render_widget(
                Paragraph::new(vec![Line::from("█"); thumb.height as usize])
                    .style(Style::default().fg(theme.foreground)),
                thumb,
            );
        }
        if inner.width > 0 && inner.height > 0 {
            f.set_cursor_position((
                inner.x.saturating_add(layout.cursor_x),
                inner.y.saturating_add(layout.cursor_y),
            ));
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
        let box_area = input_box_area(horizontal[1]);

        self.state.input_area = Some(box_area);
        let max_body_lines = box_area.height.saturating_sub(2) as usize;
        let layout = input_layout(
            &self.state.input_content,
            self.state.input_cursor,
            self.state.input_scroll,
            box_area.width.saturating_sub(2) as usize,
            max_body_lines,
            true,
        );
        self.state.input_scroll = layout.scroll;
        let inner_width = box_area
            .width
            .saturating_sub(2)
            .saturating_sub(u16::from(layout.show_scrollbar));
        let inner = Rect::new(
            box_area.x.saturating_add(1),
            box_area.y.saturating_add(1),
            inner_width,
            box_area.height.saturating_sub(2),
        );
        self.state.input_text_area = Some(inner);

        let input = Paragraph::new(
            layout
                .visible_lines
                .iter()
                .map(|line| Line::from(format!(" {line}")))
                .collect::<Vec<_>>(),
        )
        .block(
            Block::default()
                .style(Style::default().bg(theme.code_bg))
                .title(" Start a conversation ")
                .title_style(
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .style(if self.state.input_content.is_empty() {
            Style::default().fg(theme.muted).bg(theme.code_bg)
        } else {
            Style::default().fg(theme.foreground).bg(theme.code_bg)
        });

        f.render_widget(input, box_area);
        if layout.show_scrollbar {
            let track = Rect::new(box_area.right().saturating_sub(2), inner.y, 1, inner.height);
            let thumb = scrollbar_thumb(
                track,
                layout.total_lines,
                inner.height as usize,
                layout.scroll,
            );
            f.render_widget(
                Paragraph::new(vec![Line::from("│"); track.height as usize])
                    .style(Style::default().fg(theme.muted)),
                track,
            );
            f.render_widget(
                Paragraph::new(vec![Line::from("█"); thumb.height as usize])
                    .style(Style::default().fg(theme.foreground)),
                thumb,
            );
        }
        if inner.width > 0 && inner.height > 0 {
            f.set_cursor_position((
                inner.x.saturating_add(layout.cursor_x),
                inner.y.saturating_add(layout.cursor_y),
            ));
        }
    }
}

fn input_box_area(area: Rect) -> Rect {
    area.inner(Margin {
        vertical: INPUT_MARGIN_VERTICAL,
        horizontal: INPUT_MARGIN_HORIZONTAL,
    })
}

fn chat_messages_area(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y,
        area.width.saturating_sub(2),
        area.height.saturating_sub(1),
    )
}

fn bottom_input_height(state: &crate::ui::ChatTabState, area: Rect) -> u16 {
    let max_box_height = (area.height / 2).max(3);
    let margin_height = INPUT_MARGIN_VERTICAL.saturating_mul(2);
    let viewport_height = max_box_height
        .saturating_sub(2)
        .saturating_sub(margin_height) as usize;
    let layout = input_layout(
        &state.input_content,
        state.input_cursor,
        state.input_scroll,
        area.width
            .saturating_sub(3)
            .saturating_sub(INPUT_MARGIN_HORIZONTAL.saturating_mul(2)) as usize,
        viewport_height.max(1),
        true,
    );
    (layout.visible_lines.len() as u16 + 2 + margin_height).clamp(3, max_box_height)
}

fn centered_input_height(state: &crate::ui::ChatTabState, area: Rect) -> u16 {
    let width = area.width.saturating_mul(76) / 100;
    let max_box_height = (area.height / 2).max(5);
    let margin_height = INPUT_MARGIN_VERTICAL.saturating_mul(2);
    let viewport_height = max_box_height
        .saturating_sub(2)
        .saturating_sub(margin_height) as usize;
    let layout = input_layout(
        &state.input_content,
        state.input_cursor,
        state.input_scroll,
        width
            .saturating_sub(3)
            .saturating_sub(INPUT_MARGIN_HORIZONTAL.saturating_mul(2)) as usize,
        viewport_height.max(1),
        true,
    );
    (layout.visible_lines.len() as u16 + 2 + margin_height).clamp(5, max_box_height)
}

pub(crate) fn input_layout(
    content: &str,
    cursor: usize,
    scroll: usize,
    width: usize,
    viewport_height: usize,
    show_placeholder: bool,
) -> InputLayout {
    if width == 0 || viewport_height == 0 {
        return InputLayout {
            visible_lines: Vec::new(),
            line_ranges: Vec::new(),
            scroll,
            cursor_x: 0,
            cursor_y: 0,
            total_lines: 0,
            show_scrollbar: false,
        };
    }

    if content.is_empty() {
        let placeholder = if show_placeholder {
            "Type your message..."
        } else {
            ""
        };
        let visible: String = placeholder.chars().take(width).collect();
        return InputLayout {
            visible_lines: vec![visible],
            line_ranges: vec![(0, 0)],
            scroll: 0,
            cursor_x: 0,
            cursor_y: 0,
            total_lines: 1,
            show_scrollbar: false,
        };
    }

    let chars: Vec<char> = content.chars().collect();
    let cursor = cursor.min(chars.len());
    let mut lines = Vec::new();
    let mut ranges = Vec::new();
    let mut current = String::new();
    let mut start = 0usize;
    let mut column = 0usize;

    for (idx, ch) in chars.iter().enumerate() {
        if *ch == '\n' {
            lines.push(current.clone());
            ranges.push((start, idx));
            current.clear();
            start = idx + 1;
            column = 0;
            continue;
        }
        if column >= width {
            lines.push(current.clone());
            ranges.push((start, idx));
            current.clear();
            start = idx;
            column = 0;
        }
        current.push(*ch);
        column += 1;
    }
    lines.push(current);
    ranges.push((start, chars.len()));

    let cursor_line = ranges
        .iter()
        .position(|(_, end)| cursor < *end)
        .unwrap_or_else(|| ranges.len().saturating_sub(1));
    let cursor_x = cursor.saturating_sub(ranges[cursor_line].0).min(width) as u16;
    let mut scroll = scroll.min(lines.len().saturating_sub(viewport_height));
    if cursor_line < scroll {
        scroll = cursor_line;
    }
    if cursor_line >= scroll + viewport_height {
        scroll = cursor_line + 1 - viewport_height;
    }
    let end = (scroll + viewport_height).min(lines.len());
    InputLayout {
        visible_lines: lines[scroll..end].to_vec(),
        line_ranges: ranges,
        scroll,
        cursor_x,
        cursor_y: cursor_line.saturating_sub(scroll) as u16,
        total_lines: lines.len(),
        show_scrollbar: lines.len() > viewport_height,
    }
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

fn dropdown_width_for(labels: &[String], min_chars: usize, scrollbar_width: u16) -> u16 {
    let content_width = labels
        .iter()
        .map(|label| UnicodeWidthStr::width(label.as_str()))
        .max()
        .unwrap_or(1)
        .max(min_chars)
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

fn box_top_line(
    inset: usize,
    width: usize,
    title: &str,
    title_style: Style,
    fill_style: Style,
) -> Line<'static> {
    if width < 2 {
        return Line::from(Span::styled(" ".repeat(inset), fill_style));
    }

    let inner_width = width - 2;
    let mut title_text = format!(" {title} ");
    while UnicodeWidthStr::width(title_text.as_str()) > inner_width {
        title_text.pop();
    }
    let title_width = UnicodeWidthStr::width(title_text.as_str());
    let rule_width = inner_width.saturating_sub(title_width);

    let mut line = Line::from(vec![
        Span::styled(" ".repeat(inset), fill_style),
        Span::styled(" ", fill_style),
        Span::styled(title_text, title_style),
        Span::styled(" ".repeat(rule_width), fill_style),
        Span::styled(" ", fill_style),
    ]);
    line.style = fill_style;
    line
}

fn box_content_line(
    inset: usize,
    width: usize,
    line: Line<'static>,
    fill_style: Style,
) -> Line<'static> {
    let (left_pad, right_pad) = content_padding(width, &line);
    let mut spans = Vec::with_capacity(line.spans.len() + 5);
    spans.push(Span::styled(" ".repeat(inset), fill_style));
    spans.push(Span::styled("  ", fill_style));
    spans.push(Span::styled(" ".repeat(left_pad), fill_style));
    let mut remaining = width.saturating_sub(left_pad + right_pad);
    for mut span in line.spans {
        if remaining == 0 {
            break;
        }
        let mut clipped = String::new();
        for ch in span.content.chars() {
            let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if char_width > remaining {
                remaining = 0;
                break;
            }
            clipped.push(ch);
            remaining = remaining.saturating_sub(char_width);
        }
        if clipped.is_empty() {
            continue;
        }
        span.content = clipped.into();
        span.style = span.style.bg(fill_style.bg.unwrap_or(Color::Reset));
        spans.push(span);
    }
    spans.push(Span::styled(" ".repeat(right_pad), fill_style));
    spans.push(Span::styled("  ", fill_style));
    let mut line = Line::from(spans);
    line.style = fill_style;
    line
}

fn box_bottom_line(inset: usize, width: usize, fill_style: Style) -> Line<'static> {
    if width < 2 {
        return Line::from(Span::styled(" ".repeat(inset), fill_style));
    }

    let mut line = Line::from(vec![
        Span::styled(" ".repeat(inset), fill_style),
        Span::styled(" ", fill_style),
        Span::styled(" ".repeat(width - 2), fill_style),
        Span::styled(" ", fill_style),
    ]);
    line.style = fill_style;
    line
}

fn content_padding(width: usize, line: &Line<'_>) -> (usize, usize) {
    let text_width = line.width();
    let left_pad =
        alignment_padding_for(width, text_width, line.alignment.unwrap_or(Alignment::Left));
    let remaining = width.saturating_sub(text_width.min(width));
    (left_pad, remaining.saturating_sub(left_pad))
}

fn alignment_padding_for(width: usize, text_width: usize, alignment: Alignment) -> usize {
    let text_width = text_width.min(width);
    let remaining = width.saturating_sub(text_width);
    match alignment {
        Alignment::Left => 0,
        Alignment::Center => remaining / 2,
        Alignment::Right => remaining,
    }
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
    use ratatui::{Terminal, backend::TestBackend};

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
                kitty_heading_downscale: HeadingDownscale::None,
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
    fn input_layout_scrolls_to_keep_cursor_visible() {
        let layout = input_layout("abcdefghij", 10, 0, 5, 1, false);

        assert_eq!(layout.scroll, 1);
        assert_eq!(layout.cursor_x, 5);
        assert_eq!(layout.visible_lines, vec!["fghij".to_string()]);
    }

    #[test]
    fn empty_input_insets_placeholder_horizontally_without_extra_rows() {
        let layout = input_layout("", 0, 0, 40, 3, true);

        assert_eq!(
            layout.visible_lines,
            vec!["Type your message...".to_string()]
        );
        assert_eq!(layout.cursor_y, 0);
        assert_eq!(layout.total_lines, 1);
    }

    #[test]
    fn typed_input_cursor_stays_on_last_character() {
        let mut ui = crate::ui::UI::new();
        ui.tabs[0].messages.push(crate::app::message::Message::new(
            1,
            "assistant".to_string(),
            "ready".to_string(),
        ));
        ui.tabs[0].input_content = "a".to_string();
        ui.tabs[0].input_cursor = 1;
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).expect("test terminal");
        let mut chat = ChatTab::new(
            &mut ui.tabs[0],
            ChatTabProps {
                user_alignment: TextAlignment::Left,
                ai_alignment: TextAlignment::Left,
                markdown_mode: MarkdownMode::Off,
                collapse_thinking: true,
                show_chat_scrollbar: true,
                kitty_enhanced_text: false,
                kitty_heading_downscale: HeadingDownscale::None,
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
            .draw(|frame| chat.render(frame, Rect::new(0, 0, 40, 12)))
            .expect("render input");

        let input_area = chat.state.input_text_area.expect("input text area");
        terminal
            .backend_mut()
            .assert_cursor_position((input_area.x + 1, input_area.y));
    }

    #[test]
    fn bottom_input_block_has_outer_margin() {
        let mut ui = crate::ui::UI::new();
        ui.tabs[0].messages.push(crate::app::message::Message::new(
            1,
            "assistant".to_string(),
            "ready".to_string(),
        ));
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).expect("test terminal");
        let mut chat = ChatTab::new(
            &mut ui.tabs[0],
            ChatTabProps {
                user_alignment: TextAlignment::Left,
                ai_alignment: TextAlignment::Left,
                markdown_mode: MarkdownMode::Off,
                collapse_thinking: true,
                show_chat_scrollbar: true,
                kitty_enhanced_text: false,
                kitty_heading_downscale: HeadingDownscale::None,
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
            .draw(|frame| chat.render(frame, Rect::new(0, 0, 40, 12)))
            .expect("render input");

        let input_area = chat.state.input_area.expect("input area");
        assert_eq!(input_area.x, 2);
        assert_eq!(input_area.right(), 38);
        assert!(input_area.y > 0);
        assert!(input_area.bottom() < 12);
    }

    #[test]
    fn boxed_content_clips_styled_spans_to_inner_width() {
        let line = Line::from(vec![
            Span::styled("1234", Style::default().fg(Color::Red)),
            Span::styled("5678", Style::default().fg(Color::Green)),
        ]);

        let boxed = box_content_line(1, 5, line, Style::default().bg(Color::Blue));

        assert_eq!(boxed.width(), 10);
        assert_eq!(boxed.to_string(), "   12345  ");
    }

    #[test]
    fn boxed_content_background_stops_at_box_boundary() {
        let boxed = box_content_line(1, 5, Line::from("12345"), Style::default().bg(Color::Blue));
        let mut terminal = Terminal::new(TestBackend::new(20, 1)).expect("test terminal");

        terminal
            .draw(|frame| frame.render_widget(Paragraph::new(boxed), frame.area()))
            .expect("render boxed content");

        assert_eq!(terminal.backend().buffer()[(9, 0)].bg, Color::Blue);
        assert_eq!(terminal.backend().buffer()[(10, 0)].bg, Color::Reset);
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
                kitty_heading_downscale: HeadingDownscale::None,
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

        assert!(rendered.lines[anchor].to_string().contains("four"));
    }

    #[test]
    fn transcript_styles_assistant_box_and_user_clean_text() {
        let theme = crate::theme::active_theme();
        let mut ui = crate::ui::UI::new();
        ui.tabs[0].messages.push(crate::app::message::Message::new(
            1,
            "user".to_string(),
            "Question".to_string(),
        ));
        ui.tabs[0].messages.push(crate::app::message::Message::new(
            1,
            "assistant".to_string(),
            "Answer".to_string(),
        ));
        let chat = ChatTab::new(
            &mut ui.tabs[0],
            ChatTabProps {
                user_alignment: TextAlignment::Left,
                ai_alignment: TextAlignment::Left,
                markdown_mode: MarkdownMode::Off,
                collapse_thinking: true,
                show_chat_scrollbar: true,
                kitty_enhanced_text: false,
                kitty_heading_downscale: HeadingDownscale::None,
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

        let rendered = chat.build_messages(Rect::new(0, 0, 60, 12));
        let screen = rendered
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(screen.contains("Answer"));
        let user_line = rendered
            .lines
            .iter()
            .find(|line| line.to_string().contains("Question"))
            .expect("user answer line");
        assert_eq!(user_line.style.bg, Some(theme.user_bubble));
        let assistant_line = rendered
            .lines
            .iter()
            .find(|line| line.to_string().contains("Answer"))
            .expect("assistant answer line");
        assert_eq!(assistant_line.style.bg, None);
    }

    #[test]
    fn thinking_fold_uses_darker_box_style() {
        let theme = crate::theme::active_theme();
        let mut ui = crate::ui::UI::new();
        let mut assistant =
            crate::app::message::Message::new(1, "assistant".to_string(), "Answer".to_string());
        assistant.thinking_content = Some("Reason".to_string());
        ui.tabs[0].messages.push(assistant);
        let chat = ChatTab::new(
            &mut ui.tabs[0],
            ChatTabProps {
                user_alignment: TextAlignment::Left,
                ai_alignment: TextAlignment::Left,
                markdown_mode: MarkdownMode::Off,
                collapse_thinking: true,
                show_chat_scrollbar: true,
                kitty_enhanced_text: false,
                kitty_heading_downscale: HeadingDownscale::None,
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

        let rendered = chat.build_messages(Rect::new(0, 0, 60, 12));
        let thinking = rendered
            .lines
            .iter()
            .find(|line| line.to_string().contains("thinking"))
            .expect("thinking fold line");

        assert_eq!(thinking.spans[0].style.bg, Some(theme.code_bg));
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
        let mut terminal = Terminal::new(TestBackend::new(20, 8)).expect("test terminal");
        let mut chat = ChatTab::new(
            &mut ui.tabs[0],
            ChatTabProps {
                user_alignment: TextAlignment::Left,
                ai_alignment: TextAlignment::Left,
                markdown_mode: MarkdownMode::Full,
                collapse_thinking: true,
                show_chat_scrollbar: false,
                kitty_enhanced_text: false,
                kitty_heading_downscale: HeadingDownscale::None,
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
            .draw(|frame| chat.render_messages(frame, Rect::new(0, 0, 20, 8)))
            .expect("render messages");
        let screen = terminal.backend().to_string();

        // Then
        assert!(screen.contains("final tail"), "{screen}");
        let toggle_row = screen
            .lines()
            .position(|line| line.contains("show think"))
            .expect("visible thinking toggle") as u16;
        assert_eq!(chat.state.thinking_hit_areas[0].1.y, toggle_row);
    }

    #[test]
    fn kitty_heading_is_anchored_in_ratatui_cells() {
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
                kitty_heading_downscale: HeadingDownscale::None,
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
        if let Some(path) = std::env::var_os("TCUI_KITTY_HEADING_CAPTURE") {
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
            std::fs::write(path, capture).expect("write kitty heading capture");
        }

        // Then
        assert!(
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .any(|cell| cell.symbol().contains("\u{1b}]66;"))
        );
    }

    #[test]
    fn kitty_heading_falls_back_when_second_row_is_clipped() {
        let mut ui = crate::ui::UI::new();
        ui.tabs[0].messages.push(crate::app::message::Message::new(
            1,
            "assistant".to_string(),
            "# Heading".to_string(),
        ));
        let mut terminal = Terminal::new(TestBackend::new(40, 1)).expect("test terminal");
        let mut chat = ChatTab::new(
            &mut ui.tabs[0],
            ChatTabProps {
                user_alignment: TextAlignment::Left,
                ai_alignment: TextAlignment::Left,
                markdown_mode: MarkdownMode::Full,
                collapse_thinking: true,
                show_chat_scrollbar: false,
                kitty_enhanced_text: true,
                kitty_heading_downscale: HeadingDownscale::None,
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

        terminal
            .draw(|frame| chat.render_messages(frame, Rect::new(0, 0, 40, 1)))
            .expect("render clipped heading");

        assert!(
            !terminal
                .backend()
                .buffer()
                .content
                .iter()
                .any(|cell| cell.symbol().contains("\u{1b}]66;"))
        );
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
                kitty_heading_downscale: HeadingDownscale::None,
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
    fn multiline_input_grows_and_scrolls_with_long_content() {
        let mut ui = crate::ui::UI::new();
        ui.tabs[0].messages.push(crate::app::message::Message::new(
            1,
            "assistant".to_string(),
            "ready".to_string(),
        ));
        ui.tabs[0].input_content = "line one wraps around the viewport width and keeps going. line two wraps around the viewport width and keeps going. line three wraps around the viewport width and keeps going. line four wraps around the viewport width and keeps going. line five wraps around the viewport width and keeps going. line six wraps around the viewport width and keeps going.".to_string();
        ui.tabs[0].input_cursor = ui.tabs[0].input_content.chars().count();

        let mut terminal = Terminal::new(TestBackend::new(40, 16)).expect("test terminal");
        let mut chat = ChatTab::new(
            &mut ui.tabs[0],
            ChatTabProps {
                user_alignment: TextAlignment::Left,
                ai_alignment: TextAlignment::Left,
                markdown_mode: MarkdownMode::Off,
                collapse_thinking: true,
                show_chat_scrollbar: true,
                kitty_enhanced_text: false,
                kitty_heading_downscale: HeadingDownscale::None,
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
            .draw(|frame| chat.render(frame, Rect::new(0, 0, 40, 16)))
            .expect("render chat");

        let input_area = chat.state.input_area.expect("input area");
        assert!(input_area.height > 3);
        assert!(chat.state.input_scroll > 0);
    }

    #[test]
    fn dropdown_width_keeps_full_provider_and_model_labels_clickable() {
        let provider_labels = vec!["VeryLongProviderName".to_string()];
        let model_labels = vec!["deepseek-v4-flash-with-long-suffix".to_string()];

        assert_eq!(dropdown_width_for(&provider_labels, 30, 1), 33);
        assert_eq!(dropdown_width_for(&model_labels, 30, 1), 37);
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
                kitty_heading_downscale: HeadingDownscale::None,
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
        let thinking = screen.find("show thinking").expect("thinking activity");
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
