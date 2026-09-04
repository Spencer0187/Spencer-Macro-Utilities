//! Shared C++-compatible multi-instance UI state.
//!
//! `PressKey`, Wallhop, and `SpamKey` all keep at least one main instance. Removing while the main slot
//! is selected targets the final extra instance; removing an extra slot targets that selected slot.
//! Removal uses the same two-click 0.7 second confirmation window as the C++ UI.

#[derive(Clone, Copy, Debug, PartialEq)]
struct RemoveConfirmation {
    target: usize,
    remaining_seconds: f32,
}

#[derive(Clone, Debug)]
pub struct MultiInstanceState<T> {
    instances: Vec<T>,
    selected: usize,
    remove_confirmation: Option<RemoveConfirmation>,
}

impl<T: Default> Default for MultiInstanceState<T> {
    fn default() -> Self {
        Self { instances: vec![T::default()], selected: 0, remove_confirmation: None }
    }
}

impl<T> MultiInstanceState<T> {
    #[must_use]
    pub fn instances(&self) -> &[T] {
        &self.instances
    }

    pub fn instances_mut(&mut self) -> &mut [T] {
        &mut self.instances
    }

    #[must_use]
    pub const fn selected_index(&self) -> usize {
        self.selected
    }

    pub fn selected_mut(&mut self) -> &mut T {
        &mut self.instances[self.selected]
    }

    pub const fn select(&mut self, index: usize) {
        if index < self.instances.len() {
            self.selected = index;
            self.remove_confirmation = None;
        }
    }

    #[must_use]
    pub const fn removal_target(&self) -> Option<usize> {
        if self.instances.len() <= 1 {
            None
        } else if self.selected > 0 && self.selected < self.instances.len() {
            Some(self.selected)
        } else {
            Some(self.instances.len() - 1)
        }
    }

    #[must_use]
    pub fn removal_is_armed(&self) -> bool {
        self.removal_target().is_some_and(|target| {
            self.remove_confirmation.is_some_and(|confirmation| confirmation.target == target)
        })
    }

    pub fn tick_remove_confirmation(&mut self, delta_seconds: f32, section_selected: bool) {
        let Some(mut confirmation) = self.remove_confirmation else {
            return;
        };
        if !section_selected || self.removal_target() != Some(confirmation.target) {
            self.remove_confirmation = None;
            return;
        }
        confirmation.remaining_seconds -= delta_seconds.max(0.0);
        self.remove_confirmation = (confirmation.remaining_seconds > 0.0).then_some(confirmation);
    }

    /// First click arms removal; a second click inside the confirmation window removes the C++ target.
    /// Returns the removed index only on the confirming click.
    pub fn click_remove(&mut self) -> Option<usize> {
        let target = self.removal_target()?;
        if self.removal_is_armed() {
            self.instances.remove(target);
            self.selected = self.selected.min(self.instances.len() - 1);
            self.remove_confirmation = None;
            return Some(target);
        }
        self.remove_confirmation = Some(RemoveConfirmation { target, remaining_seconds: 0.7 });
        None
    }
}

impl<T: Default> MultiInstanceState<T> {
    pub fn add_default(&mut self) {
        self.instances.push(T::default());
        self.remove_confirmation = None;
    }
}

#[cfg(test)]
mod tests {
    use super::MultiInstanceState;

    #[test]
    fn main_slot_removes_last_extra_after_confirmation() {
        let mut state = MultiInstanceState::<i32>::default();
        state.add_default();
        state.add_default();
        state.instances_mut().copy_from_slice(&[10, 20, 30]);
        assert_eq!(state.removal_target(), Some(2));
        assert_eq!(state.click_remove(), None);
        assert!(state.removal_is_armed());
        assert_eq!(state.click_remove(), Some(2));
        assert_eq!(state.instances(), [10, 20]);
        assert_eq!(state.selected_index(), 0);
    }

    #[test]
    fn extra_slot_removes_selected_and_shifts_later_values_left() {
        let mut state = MultiInstanceState::<i32>::default();
        state.add_default();
        state.add_default();
        state.instances_mut().copy_from_slice(&[10, 20, 30]);
        state.select(1);
        assert_eq!(state.removal_target(), Some(1));
        let _ = state.click_remove();
        assert_eq!(state.click_remove(), Some(1));
        assert_eq!(state.instances(), [10, 30]);
        assert_eq!(state.selected_index(), 1);
    }

    #[test]
    fn confirmation_expires_or_resets_when_selection_changes() {
        let mut state = MultiInstanceState::<i32>::default();
        state.add_default();
        let _ = state.click_remove();
        state.tick_remove_confirmation(0.701, true);
        assert!(!state.removal_is_armed());
        let _ = state.click_remove();
        state.select(1);
        assert!(!state.removal_is_armed());
        let _ = state.click_remove();
        state.tick_remove_confirmation(0.1, false);
        assert!(!state.removal_is_armed());
    }

    #[test]
    fn one_instance_cannot_be_removed() {
        let mut state = MultiInstanceState::<i32>::default();
        assert_eq!(state.removal_target(), None);
        assert_eq!(state.click_remove(), None);
        assert_eq!(state.instances().len(), 1);
    }
}
