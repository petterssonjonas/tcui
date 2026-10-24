use ratatui::layout::{Position, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VaultMouseAction {
    TreeRow(usize),
    FocusTree,
    FocusViewer,
    FocusEditor,
    EditSelected,
    Confirm,
    Cancel,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct VaultHitAreas {
    areas: Vec<(Rect, VaultMouseAction)>,
}

impl VaultHitAreas {
    pub(crate) fn clear(&mut self) {
        self.areas.clear();
    }

    pub(crate) fn push(&mut self, area: Rect, action: VaultMouseAction) {
        if area.width > 0 && area.height > 0 {
            self.areas.push((area, action));
        }
    }

    pub(crate) fn action_at(&self, position: Position) -> Option<VaultMouseAction> {
        self.areas
            .iter()
            .rev()
            .find_map(|(area, action)| area.contains(position).then_some(*action))
    }

    pub(crate) fn area(&self, action: VaultMouseAction) -> Option<Rect> {
        self.areas
            .iter()
            .find_map(|(area, candidate)| (*candidate == action).then_some(*area))
    }

    #[cfg(test)]
    pub(crate) fn area_for(&self, predicate: impl Fn(VaultMouseAction) -> bool) -> Option<Rect> {
        self.areas
            .iter()
            .find_map(|(area, action)| predicate(*action).then_some(*area))
    }
}
