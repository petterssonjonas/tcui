#![allow(dead_code)]
use crossterm::event::KeyEvent;

pub mod handler;

#[derive(Debug, Clone)]
pub enum Event {
    Input(KeyEvent),
    Resize(u16, u16),
    Tick,
}
