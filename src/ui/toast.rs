use ratatui::{
    prelude::*,
    widgets::{Block, Clear, Paragraph},
    Frame,
};
use std::collections::VecDeque;

use crate::config::ToastPosition;

const MAX_TOASTS: usize = 5;
const DEFAULT_DURATION_TICKS: u64 = 180;
const TOAST_HEIGHT: u16 = 3;
const TOAST_GAP: u16 = 1;
pub(super) const TOAST_MARGIN: u16 = 2;

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
    pub level: ToastLevel,
    pub created_tick: u64,
    pub duration_ticks: u64,
}

impl Toast {
    pub fn new(message: String, frame_tick: u64) -> Self {
        Self {
            message,
            level: ToastLevel::Info,
            created_tick: frame_tick,
            duration_ticks: DEFAULT_DURATION_TICKS,
        }
    }

    pub fn with_level(
        message: String,
        level: ToastLevel,
        frame_tick: u64,
        duration_ticks: u64,
    ) -> Self {
        Self {
            message,
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

    let stack_height = stack
        .len()
        .saturating_mul(TOAST_HEIGHT as usize)
        .saturating_add(
            stack
                .len()
                .saturating_sub(1)
                .saturating_mul(TOAST_GAP as usize),
        );
    let mut y = if position == ToastPosition::Center {
        area.y
            .saturating_add(area.height.saturating_sub(stack_height as u16) / 2)
    } else {
        area.y.saturating_add(TOAST_MARGIN)
    };
    for toast in stack.visible() {
        let Some(toast_area) = toast_rect(area, toast, position, right_sidebar_width, y) else {
            break;
        };
        render_one(f, toast_area, toast);
        y = y.saturating_add(TOAST_HEIGHT + TOAST_GAP);
        if y.saturating_add(TOAST_HEIGHT) > area.bottom() {
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
    let title_area = Rect::new(toast_area.x, toast_area.y, toast_area.width, 1);
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
    let inner = Rect::new(
        toast_area.x + 1,
        toast_area.y + 1,
        toast_area.width.saturating_sub(2),
        1,
    );
    f.render_widget(
        Paragraph::new(trim_message(
            &toast.message,
            toast_area.width.saturating_sub(4) as usize,
        ))
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
    let available_width = right_limit.saturating_sub(area.x.saturating_add(TOAST_MARGIN));
    if available_width < 20 {
        return None;
    }
    let max_message_width = available_width.saturating_sub(6).clamp(16, 68) as usize;
    let message_width =
        unicode_width::UnicodeWidthStr::width(toast.message.as_str()).min(max_message_width) + 4;
    let width = (message_width as u16).min(available_width);
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
    Some(Rect::new(x, y, width, TOAST_HEIGHT))
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

fn trim_message(message: &str, max_width: usize) -> String {
    if unicode_width::UnicodeWidthStr::width(message) <= max_width {
        return message.to_string();
    }
    let mut trimmed = message
        .chars()
        .take(max_width.saturating_sub(1))
        .collect::<String>();
    trimmed.push('~');
    trimmed
}
