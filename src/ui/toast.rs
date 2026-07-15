use ratatui::{
    prelude::*,
    widgets::{Block, Clear, Paragraph},
    Frame,
};
use std::collections::VecDeque;

use crate::config::app_config::{HeadingDownscale, MarkdownMode};
use crate::config::ToastPosition;
use crate::ui::components::markdown::MarkdownRenderer;
use crate::ui::components::terminal_capabilities::{TerminalCapabilities, TerminalKind};

const MAX_TOASTS: usize = 5;
const DEFAULT_DURATION_TICKS: u64 = 180;
const TOAST_MESSAGE_WIDTH: usize = 36;
const MAX_TOAST_LINES: usize = 6;
const TOAST_INSET: u16 = 1;
const TOAST_TITLE_ROWS: u16 = 1;
const TOAST_BOTTOM_PAD: u16 = 1;
const TOAST_GAP: u16 = 1;
pub(super) const TOAST_MARGIN: u16 = 2;

fn toast_width() -> u16 {
    TOAST_MESSAGE_WIDTH as u16 + TOAST_INSET * 2
}

pub(super) fn toast_height(line_count: usize) -> u16 {
    TOAST_TITLE_ROWS + line_count.max(1) as u16 + TOAST_BOTTOM_PAD
}

fn kitty_disabled_capabilities() -> TerminalCapabilities {
    TerminalCapabilities {
        terminal: TerminalKind::Unknown,
        multiplexer: None,
        kitty_graphics: false,
        kitty_text_sizing: false,
        tmux_passthrough: false,
    }
}

fn build_lines(message: &str) -> Vec<Line<'static>> {
    let renderer = MarkdownRenderer::new(kitty_disabled_capabilities());
    let mut lines: Vec<Line<'static>> = Vec::new();
    for segment in message.split('\n') {
        if segment.is_empty() {
            lines.push(Line::raw(""));
        } else {
            let rendered = renderer.render(
                segment,
                MarkdownMode::Full,
                TOAST_MESSAGE_WIDTH,
                false,
                HeadingDownscale::None,
                false,
            );
            lines.extend(rendered.lines);
        }
        if lines.len() >= MAX_TOAST_LINES {
            lines.truncate(MAX_TOAST_LINES);
            return lines;
        }
    }
    lines
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub message: String,
    pub lines: Vec<Line<'static>>,
    pub level: ToastLevel,
    pub created_tick: u64,
    pub duration_ticks: u64,
}

impl Toast {
    pub fn new(message: String, frame_tick: u64) -> Self {
        Self::with_level(
            message,
            ToastLevel::Info,
            frame_tick,
            DEFAULT_DURATION_TICKS,
        )
    }

    pub fn with_level(
        message: String,
        level: ToastLevel,
        frame_tick: u64,
        duration_ticks: u64,
    ) -> Self {
        let lines = build_lines(&message);
        Self {
            message,
            lines,
            level,
            created_tick: frame_tick,
            duration_ticks,
        }
    }

    fn expires_at(&self) -> u64 {
        self.created_tick.saturating_add(self.duration_ticks)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ToastStack {
    toasts: VecDeque<Toast>,
}

impl ToastStack {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, toast: Toast) {
        if self.toasts.len() == MAX_TOASTS {
            self.toasts.pop_front();
        }
        self.toasts.push_back(toast);
    }

    pub fn push_message(&mut self, message: String, frame_tick: u64) {
        self.push(Toast::new(message, frame_tick));
    }

    pub fn expire(&mut self, frame_tick: u64) {
        self.toasts.retain(|toast| frame_tick < toast.expires_at());
    }

    pub fn len(&self) -> usize {
        self.toasts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    fn visible(&self) -> impl DoubleEndedIterator<Item = &Toast> {
        self.toasts.iter().rev()
    }
}

pub fn render(
    f: &mut Frame,
    area: Rect,
    stack: &mut ToastStack,
    frame_tick: u64,
    position: ToastPosition,
    right_sidebar_width: u16,
) {
    if position == ToastPosition::Off {
        return;
    }
    stack.expire(frame_tick);
    if stack.is_empty() || area.width < 24 || area.height < 6 {
        return;
    }

    let toasts: Vec<&Toast> = stack.visible().collect();
    let stack_height: u16 = toasts
        .iter()
        .map(|toast| toast_height(toast.lines.len().max(1)))
        .sum::<u16>()
        .saturating_add(TOAST_GAP.saturating_mul(toasts.len().saturating_sub(1) as u16));

    let mut y = if position == ToastPosition::Center {
        area.y
            .saturating_add(area.height.saturating_sub(stack_height) / 2)
    } else {
        area.y.saturating_add(TOAST_MARGIN)
    };
    for toast in toasts {
        let Some(toast_area) = toast_rect(area, toast, position, right_sidebar_width, y) else {
            break;
        };
        render_one(f, toast_area, toast);
        y = y.saturating_add(toast_area.height + TOAST_GAP);
        if y >= area.bottom() {
            break;
        }
    }
}

fn render_one(f: &mut Frame, toast_area: Rect, toast: &Toast) {
    let theme = crate::theme::active_theme();
    f.render_widget(Clear, toast_area);
    f.render_widget(
        Block::default().style(Style::default().bg(theme.panel)),
        toast_area,
    );
    let title_area = Rect::new(
        toast_area.x,
        toast_area.y,
        toast_area.width,
        TOAST_TITLE_ROWS,
    );
    f.render_widget(
        Paragraph::new(toast_title(toast.level))
            .style(
                Style::default()
                    .fg(level_color(toast.level))
                    .bg(theme.panel)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Left),
        title_area,
    );
    let line_count = toast.lines.len().max(1) as u16;
    let inner = Rect::new(
        toast_area.x + TOAST_INSET,
        toast_area.y + TOAST_TITLE_ROWS,
        toast_area.width.saturating_sub(TOAST_INSET * 2),
        line_count,
    );
    f.render_widget(
        Paragraph::new(toast.lines.clone())
            .style(Style::default().fg(theme.foreground).bg(theme.panel)),
        inner,
    );
}

pub(super) fn toast_rect(
    area: Rect,
    toast: &Toast,
    position: ToastPosition,
    right_sidebar_width: u16,
    y: u16,
) -> Option<Rect> {
    let right_limit = area.right().saturating_sub(if right_sidebar_width > 0 {
        right_sidebar_width.saturating_add(TOAST_MARGIN)
    } else {
        TOAST_MARGIN
    });
    let width = toast_width();
    let available_width = right_limit.saturating_sub(area.x.saturating_add(TOAST_MARGIN));
    if available_width < width {
        return None;
    }
    let height = toast_height(toast.lines.len().max(1));
    let x = match position {
        ToastPosition::TopLeft => area.x.saturating_add(TOAST_MARGIN),
        ToastPosition::TopCenter | ToastPosition::Center => area.x.saturating_add(
            area.width
                .saturating_sub(right_sidebar_width)
                .saturating_sub(width)
                / 2,
        ),
        ToastPosition::TopRight => right_limit.saturating_sub(width),
        ToastPosition::Off => return None,
    };
    Some(Rect::new(x, y, width, height))
}

fn toast_title(level: ToastLevel) -> &'static str {
    match level {
        ToastLevel::Info => " Notice ",
        ToastLevel::Success => " Success ",
        ToastLevel::Warning => " Warning ",
        ToastLevel::Error => " Error ",
    }
}

fn level_color(level: ToastLevel) -> Color {
    let theme = crate::theme::active_theme();
    match level {
        ToastLevel::Info => theme.accent,
        ToastLevel::Success => theme.success,
        ToastLevel::Warning => theme.warning,
        ToastLevel::Error => theme.error,
    }
}
