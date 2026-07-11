//! Focus management primitives for the ratatui-based TUI.
//!
//! `Focus` enumerates the surfaces that can own input focus, and `FocusStack`
//! tracks the active focus layering so that `Esc` can restore the prior
//! surface. The host app owns a `FocusStack` and consults `top()` before
//! routing key/mouse events.
//!
//! Overlay trait will land when the first popup uses it (Todo 3+); it is not
//! introduced here because it would have zero implementors.

#![allow(dead_code)]

/// The surfaces that can own focus in the TUI.
///
/// Variants are ordered roughly bottom-to-top: `Chat` is the base layer, the
/// sidebars sit alongside it, and the remaining variants are overlays that
/// stack on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Focus {
    #[default]
    Chat,
    LeftSidebar,
    RightSidebar,
    CommandPalette,
    SettingsPanel,
    KeybindCapture,
    ConfirmModal,
    ArtifactViewer,
    EditorPopup,
    ListPopup,
}

/// A host-owned stack of [`Focus`] layers.
///
/// The bottom of the stack is the base surface (typically `Focus::Chat`);
/// `top()` is the currently focused surface. Overlays `push` on open and
/// `pop` on close; `pop_until` restores focus to a known lower layer on
/// `Esc`.
#[derive(Debug, Default, Clone)]
pub struct FocusStack {
    layers: Vec<Focus>,
}

impl FocusStack {
    /// Create an empty stack. Callers typically `push(Focus::Chat)` first.
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    /// Push a new focus layer on top.
    pub fn push(&mut self, focus: Focus) {
        self.layers.push(focus);
    }

    /// Pop and return the top focus layer, or `None` if the stack is empty.
    pub fn pop(&mut self) -> Option<Focus> {
        self.layers.pop()
    }

    /// The currently focused surface, or `None` if the stack is empty.
    pub fn top(&self) -> Option<&Focus> {
        self.layers.last()
    }

    /// Pop layers until `target` is the top surface, restoring focus to it.
    ///
    /// This is the strict-`Esc` helper: closing the top overlay returns focus
    /// to the layer below. If `target` is already on top this is a no-op; if
    /// `target` is not present the stack is emptied (caller must pass a valid
    /// lower layer).
    pub fn pop_until(&mut self, target: &Focus) {
        while let Some(top) = self.layers.last() {
            if top == target {
                return;
            }
            self.layers.pop();
        }
    }

    /// The number of layers currently on the stack.
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    /// Whether the stack holds no layers.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_then_top_returns_pushed_focus() {
        let mut stack = FocusStack::new();
        stack.push(Focus::Chat);
        assert_eq!(stack.top(), Some(&Focus::Chat));
        assert_eq!(stack.len(), 1);
        assert!(!stack.is_empty());
    }

    #[test]
    fn pop_empty_stack_returns_none() {
        let mut stack = FocusStack::new();
        assert!(stack.is_empty());
        assert_eq!(stack.pop(), None);
        assert_eq!(stack.top(), None);
    }

    #[test]
    fn push_then_pop_returns_last_pushed() {
        let mut stack = FocusStack::new();
        stack.push(Focus::Chat);
        stack.push(Focus::CommandPalette);
        assert_eq!(stack.top(), Some(&Focus::CommandPalette));
        assert_eq!(stack.pop(), Some(Focus::CommandPalette));
        assert_eq!(stack.top(), Some(&Focus::Chat));
    }

    #[test]
    fn push_pop_restore_roundtrip() {
        // push Chat, then push CommandPalette, pop returns CommandPalette
        // and top is Chat again.
        let mut stack = FocusStack::new();
        stack.push(Focus::Chat);
        stack.push(Focus::CommandPalette);
        let popped = stack.pop();
        assert_eq!(popped, Some(Focus::CommandPalette));
        assert_eq!(stack.top(), Some(&Focus::Chat));
    }

    #[test]
    fn pop_until_restores_lower_layer() {
        // strict-Esc: from [Chat, CommandPalette], pop_until(Chat) pops
        // back one level and leaves Chat on top.
        let mut stack = FocusStack::new();
        stack.push(Focus::Chat);
        stack.push(Focus::CommandPalette);
        stack.pop_until(&Focus::Chat);
        assert_eq!(stack.top(), Some(&Focus::Chat));
        assert_eq!(stack.len(), 1);
    }

    #[test]
    fn pop_until_noop_when_target_on_top() {
        let mut stack = FocusStack::new();
        stack.push(Focus::Chat);
        stack.push(Focus::SettingsPanel);
        // target is already top -> no pop
        stack.pop_until(&Focus::SettingsPanel);
        assert_eq!(stack.top(), Some(&Focus::SettingsPanel));
        assert_eq!(stack.len(), 2);
    }

    #[test]
    fn pop_until_empties_when_target_absent() {
        let mut stack = FocusStack::new();
        stack.push(Focus::Chat);
        stack.push(Focus::CommandPalette);
        stack.pop_until(&Focus::EditorPopup);
        assert!(stack.is_empty());
    }

    #[test]
    fn default_focus_is_chat() {
        assert_eq!(Focus::default(), Focus::Chat);
    }

    #[test]
    fn default_focusstack_is_empty() {
        let stack = FocusStack::default();
        assert!(stack.is_empty());
        assert_eq!(stack.len(), 0);
    }
}
