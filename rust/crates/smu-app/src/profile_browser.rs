use crate::theme::ThemeDefinition;
use dear_imgui_rs::{
    Condition, MouseButton, SelectableFlags, StyleColor, StyleVar, Ui, WindowFlags,
};
use smu_profile::{DEFAULT_PROFILE_NAME, ProfileCatalog, generate_new_profile_name};

const MENU_WIDTH: f32 = 260.0;
const MENU_MAX_HEIGHT: f32 = 300.0;
const DELETE_CONFIRM_SECONDS: f32 = 0.7;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProfileAction {
    SaveTo { name: String },
    Load { name: String },
    Duplicate { source: String },
    Rename { old_name: String, new_name: String },
    Delete { name: String },
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Default)]
pub struct ProfileBrowserState {
    expanded: bool,
    names: Vec<String>,
    preferred_profile: Option<String>,
    current_loaded_profile: Option<String>,
    loaded_from_backup: bool,
    source_available: bool,
    selected_index: Option<usize>,
    editing_index: Option<usize>,
    edit_buffer: String,
    rename_error: String,
    delete_target: Option<usize>,
    delete_timer: f32,
    delete_loaded_name_at_confirm: Option<String>,
    refresh_requested: bool,
    last_saved_selection_name: String,
}

impl ProfileBrowserState {
    pub fn apply_parity_reference(&mut self) {
        self.expanded = true;
        self.set_current_loaded_profile(Some(DEFAULT_PROFILE_NAME));
        self.set_catalog(
            Some(ProfileCatalog {
                names: vec![DEFAULT_PROFILE_NAME.to_owned()],
                preferred_profile: Some(DEFAULT_PROFILE_NAME.to_owned()),
                loaded_from_backup: false,
            }),
            Some(DEFAULT_PROFILE_NAME),
        );
        self.source_available = true;
        self.refresh_requested = false;
    }

    #[must_use]
    pub const fn refresh_requested(&self) -> bool {
        self.refresh_requested
    }

    #[must_use]
    pub fn take_selection_autosave_request(&mut self, current_loaded: Option<&str>) -> bool {
        if !self.expanded {
            return false;
        }
        let selected = self.selected_name().cloned().unwrap_or_default();
        if selected == self.last_saved_selection_name {
            return false;
        }
        let should_save = !self.last_saved_selection_name.is_empty()
            && current_loaded.is_some_and(|name| !name.is_empty() && name != DEFAULT_PROFILE_NAME);
        self.last_saved_selection_name = selected;
        should_save
    }

    pub const fn clear_refresh_request(&mut self) {
        self.refresh_requested = false;
    }

    pub fn set_catalog(&mut self, catalog: Option<ProfileCatalog>, select_name: Option<&str>) {
        let previous_selection =
            select_name.map(str::to_owned).or_else(|| self.selected_name().cloned());
        if let Some(catalog) = catalog {
            self.names = catalog.names;
            self.preferred_profile = catalog.preferred_profile;
            self.loaded_from_backup = catalog.loaded_from_backup;
            self.source_available = true;
        } else {
            self.names.clear();
            self.preferred_profile = None;
            self.loaded_from_backup = false;
            self.source_available = false;
        }
        self.selected_index = previous_selection
            .as_deref()
            .and_then(|name| self.names.iter().position(|candidate| candidate == name))
            .or_else(|| {
                self.current_loaded_profile
                    .as_deref()
                    .and_then(|name| self.names.iter().position(|candidate| candidate == name))
            })
            .or_else(|| {
                self.preferred_profile
                    .as_deref()
                    .and_then(|name| self.names.iter().position(|candidate| candidate == name))
            });
        self.editing_index = None;
        self.rename_error.clear();
        self.reset_delete_confirmation();
    }

    fn selected_name(&self) -> Option<&String> {
        self.selected_index.and_then(|index| self.names.get(index))
    }

    pub fn set_current_loaded_profile(&mut self, name: Option<&str>) {
        self.current_loaded_profile = name.map(str::to_owned);
        if self.delete_loaded_name_at_confirm.as_deref() != self.current_loaded_profile.as_deref() {
            self.reset_delete_confirmation();
        }
    }

    const fn request_refresh(&mut self) {
        self.refresh_requested = true;
    }

    fn reset_delete_confirmation(&mut self) {
        self.delete_target = None;
        self.delete_timer = 0.0;
        self.delete_loaded_name_at_confirm = None;
    }

    fn tick_delete_confirmation(&mut self, delta_time: f32) {
        if self.delete_target.is_none() {
            return;
        }
        self.delete_timer -= delta_time;
        if self.delete_timer <= 0.0
            || self.delete_target != self.selected_index
            || self.delete_loaded_name_at_confirm.as_deref()
                != self.current_loaded_profile.as_deref()
        {
            self.reset_delete_confirmation();
        }
    }
}

pub fn render(
    ui: &Ui,
    state: &mut ProfileBrowserState,
    button_width: f32,
    theme: &ThemeDefinition,
) -> Option<ProfileAction> {
    let label = if state.expanded { "Profiles <-" } else { "Profiles ->" };
    if ui.button_with_size(label, [button_width, 0.0]) {
        state.expanded = !state.expanded;
        if state.expanded {
            state.request_refresh();
            state.rename_error.clear();
        } else {
            state.editing_index = None;
        }
    }
    let button_position = ui.item_rect_min();
    if !state.expanded {
        return None;
    }

    state.tick_delete_confirmation(ui.io().delta_time());
    let style = ui.clone_style();
    let item_height = ui.text_line_height_with_spacing();
    let shown = u16::try_from(if state.names.is_empty() { 3 } else { state.names.len().min(10) })
        .unwrap_or(10);
    let buttons_height = style.item_spacing()[1].mul_add(2.0, ui.frame_height_with_spacing() * 2.0);
    let list_height = style.window_padding()[1].mul_add(2.0, f32::from(shown) * item_height);
    let menu_height =
        (buttons_height + list_height + style.separator_text_align()[1]).min(MENU_MAX_HEIGHT);
    let position = [
        button_position[0] - 135.0,
        button_position[1] - menu_height - style.window_padding()[1] + 3.0,
    ];

    let popup_padding = ui.push_style_var(StyleVar::WindowPadding([5.0, 5.0]));
    let mut action = None;
    ui.window("##ProfilesDropUpMenu")
        .position(position, Condition::Always)
        .size([MENU_WIDTH, menu_height], Condition::Always)
        .flags(
            WindowFlags::NO_TITLE_BAR
                | WindowFlags::NO_MOVE
                | WindowFlags::NO_RESIZE
                | WindowFlags::NO_SAVED_SETTINGS,
        )
        .build(|| {
            action = render_menu_contents(ui, state, theme);
        });
    drop(popup_padding);
    action
}

fn render_menu_contents(
    ui: &Ui,
    state: &mut ProfileBrowserState,
    theme: &ThemeDefinition,
) -> Option<ProfileAction> {
    let selected_name = state.selected_name().cloned();
    let action_width =
        ui.clone_style().item_spacing()[0].mul_add(-2.0, ui.content_region_avail()[0]) / 3.0;

    let save_target = selected_name
        .as_deref()
        .filter(|name| *name != DEFAULT_PROFILE_NAME)
        .map_or_else(|| generate_new_profile_name(&state.names), str::to_owned);
    if ui.button_with_size("Save To", [action_width, 0.0]) {
        return Some(ProfileAction::SaveTo { name: save_target });
    }
    ui.same_line();
    let disabled_load = selected_name.is_none().then(|| ui.begin_disabled());
    let load_clicked = ui.button_with_size("Load", [action_width, 0.0]);
    drop(disabled_load);
    if load_clicked && let Some(name) = selected_name.clone() {
        return Some(ProfileAction::Load { name });
    }
    ui.same_line();

    let can_delete = selected_name.is_some();
    let delete_disabled = (!can_delete).then(|| ui.begin_disabled());
    let confirming = state.delete_target == state.selected_index && state.delete_target.is_some();
    let delete_color =
        confirming.then(|| ui.push_style_color(StyleColor::Button, [1.0, 1.0, 0.0, 1.0]));
    let delete_text =
        confirming.then(|| ui.push_style_color(StyleColor::Text, [0.0, 0.0, 0.0, 1.0]));
    let delete_clicked = ui.button_with_size("Delete", [action_width, 0.0]);
    drop(delete_text);
    drop(delete_color);
    drop(delete_disabled);
    if delete_clicked && can_delete {
        if confirming {
            let name = selected_name.unwrap_or_default();
            state.reset_delete_confirmation();
            return Some(ProfileAction::Delete { name });
        }
        state.delete_target = state.selected_index;
        state.delete_timer = DELETE_CONFIRM_SECONDS;
        state.delete_loaded_name_at_confirm = state.current_loaded_profile.clone();
    }

    let duplicate_allowed = selected_name.is_some();
    let duplicate_disabled = (!duplicate_allowed).then(|| ui.begin_disabled());
    let duplicate_clicked = ui.button_with_size("Duplicate", [-f32::MIN_POSITIVE, 0.0]);
    drop(duplicate_disabled);
    if duplicate_clicked && duplicate_allowed {
        return Some(ProfileAction::Duplicate { source: selected_name.unwrap_or_default() });
    }
    crate::parity::separator(ui);

    if state.loaded_from_backup {
        ui.text_colored(
            theme.warning_color,
            "Catalog is being read from the .bak file; no repair was attempted.",
        );
    }
    if !state.source_available {
        ui.text_disabled(
            "No existing settings source was found. The Rust browser will not create one yet.",
        );
    }

    let available_height = ui.content_region_avail()[1]
        - ui.clone_style().item_spacing()[1]
        - if state.rename_error.is_empty() { 0.0 } else { ui.text_line_height_with_spacing() };
    let mut pending_action = None;
    ui.child_window("##ProfilesOptionsList")
        .size([0.0, available_height.max(1.0)])
        .border(true)
        .flags(WindowFlags::HORIZONTAL_SCROLLBAR)
        .build(ui, || {
            pending_action = render_profile_list(ui, state);
        });
    if !state.rename_error.is_empty() {
        ui.text_colored([1.0, 0.0, 0.0, 1.0], &state.rename_error);
    }
    pending_action
}

fn render_profile_list(ui: &Ui, state: &mut ProfileBrowserState) -> Option<ProfileAction> {
    for index in 0..state.names.len() {
        let name = state.names[index].clone();
        let selected = state.selected_index == Some(index);
        let currently_loaded = state.current_loaded_profile.as_deref() == Some(name.as_str());
        let default_profile = name == DEFAULT_PROFILE_NAME;

        let loaded_color =
            currently_loaded.then(|| ui.push_style_color(StyleColor::Text, [0.0, 1.0, 0.0, 1.0]));
        let default_color =
            default_profile.then(|| ui.push_style_color(StyleColor::Text, [1.0, 0.54, 0.0, 1.0]));
        let mut action = None;
        if state.editing_index == Some(index) {
            ui.set_keyboard_focus_here();
            ui.set_next_item_width(-f32::MIN_POSITIVE);
            let enter = ui
                .input_text("##EditProfileName", &mut state.edit_buffer)
                .enter_returns_true(true)
                .auto_select_all(true)
                .build();
            let click_away = ui.is_mouse_clicked(MouseButton::Left) && !ui.is_item_hovered();
            if enter || click_away {
                action = finish_rename(state, index, &name);
            }
        } else if ui
            .selectable_config(&name)
            .selected(selected)
            .flags(SelectableFlags::ALLOW_DOUBLE_CLICK)
            .build()
        {
            state.selected_index = Some(index);
            state.reset_delete_confirmation();
            if ui.is_mouse_double_clicked(MouseButton::Left) && !default_profile {
                state.editing_index = Some(index);
                state.edit_buffer.clone_from(&name);
                state.rename_error.clear();
            }
        }
        drop(default_color);
        drop(loaded_color);
        if action.is_some() {
            return action;
        }
    }
    None
}

fn finish_rename(
    state: &mut ProfileBrowserState,
    index: usize,
    old_name: &str,
) -> Option<ProfileAction> {
    let new_name = state.edit_buffer.clone();
    state.rename_error.clear();
    if new_name == DEFAULT_PROFILE_NAME {
        "Name cannot be default.".clone_into(&mut state.rename_error);
        return None;
    }
    if new_name.is_empty() {
        "Name cannot be empty.".clone_into(&mut state.rename_error);
        return None;
    }
    if state.names.iter().enumerate().any(|(other, name)| other != index && name == &new_name) {
        "Name already exists.".clone_into(&mut state.rename_error);
        return None;
    }
    state.editing_index = None;
    Some(ProfileAction::Rename { old_name: old_name.to_owned(), new_name })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{ProfileBrowserState, finish_rename};
    use smu_profile::{DEFAULT_PROFILE_NAME, ProfileCatalog};

    #[test]
    fn parity_reference_is_isolated_default_catalog() {
        let mut state = ProfileBrowserState::default();
        state.apply_parity_reference();
        assert!(state.expanded);
        assert_eq!(state.names, vec![DEFAULT_PROFILE_NAME.to_owned()]);
        assert_eq!(state.selected_index, Some(0));
        assert_eq!(state.current_loaded_profile.as_deref(), Some(DEFAULT_PROFILE_NAME));
        assert!(state.source_available);
        assert!(!state.loaded_from_backup);
        assert!(!state.refresh_requested);
    }

    #[test]
    fn rename_validation_rejects_default_empty_and_duplicates() {
        let mut state = ProfileBrowserState {
            names: vec!["Profile 1".to_owned(), "Profile 2".to_owned()],
            edit_buffer: "(default)".to_owned(),
            ..ProfileBrowserState::default()
        };
        assert!(finish_rename(&mut state, 0, "Profile 1").is_none());
        assert_eq!(state.rename_error, "Name cannot be default.");
        state.edit_buffer.clear();
        assert!(finish_rename(&mut state, 0, "Profile 1").is_none());
        assert_eq!(state.rename_error, "Name cannot be empty.");
        state.edit_buffer = "Profile 2".to_owned();
        assert!(finish_rename(&mut state, 0, "Profile 1").is_none());
        assert_eq!(state.rename_error, "Name already exists.");
    }

    #[test]
    fn expanded_selection_change_autosaves_only_after_first_selection_is_recorded() {
        let mut state = ProfileBrowserState {
            expanded: true,
            names: vec!["Profile 1".to_owned(), "Profile 2".to_owned()],
            selected_index: Some(0),
            ..ProfileBrowserState::default()
        };
        assert!(!state.take_selection_autosave_request(Some("Profile 1")));
        state.selected_index = Some(1);
        assert!(state.take_selection_autosave_request(Some("Profile 1")));
        assert!(!state.take_selection_autosave_request(Some("Profile 1")));
        state.selected_index = Some(0);
        assert!(!state.take_selection_autosave_request(Some(DEFAULT_PROFILE_NAME)));
    }

    #[test]
    fn current_loaded_profile_overrides_metadata_preference_for_selection_and_delete_guard() {
        let mut state = ProfileBrowserState::default();
        state.set_current_loaded_profile(Some("Profile 1"));
        state.set_catalog(
            Some(ProfileCatalog {
                names: vec!["Profile 1".to_owned(), "Profile 2".to_owned()],
                preferred_profile: Some("Profile 2".to_owned()),
                loaded_from_backup: false,
            }),
            None,
        );
        assert_eq!(state.selected_name().map(String::as_str), Some("Profile 1"));
        state.delete_target = Some(0);
        state.delete_timer = 0.7;
        state.delete_loaded_name_at_confirm = Some("Profile 1".to_owned());
        state.set_current_loaded_profile(Some("Profile 2"));
        assert!(state.delete_target.is_none());
    }

    #[test]
    fn catalog_refresh_preserves_selection_then_prefers_resume_target() {
        let mut state = ProfileBrowserState::default();
        state.set_catalog(
            Some(ProfileCatalog {
                names: vec!["(default)".to_owned(), "Profile 2".to_owned()],
                preferred_profile: Some("Profile 2".to_owned()),
                loaded_from_backup: false,
            }),
            None,
        );
        assert_eq!(state.selected_name().map(String::as_str), Some("Profile 2"));
        state.selected_index = Some(0);
        state.set_catalog(
            Some(ProfileCatalog {
                names: vec!["(default)".to_owned(), "Profile 2".to_owned(), "Profile 3".to_owned()],
                preferred_profile: Some("Profile 3".to_owned()),
                loaded_from_backup: false,
            }),
            None,
        );
        assert_eq!(state.selected_name().map(String::as_str), Some("(default)"));
    }
}
