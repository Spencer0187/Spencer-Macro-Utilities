use dear_imgui_rs::{Condition, StyleVar, Ui, WindowFlags};
use serde_json::{Map, Value};
use smu_profile::payload::{PreservedProfilePayload, ProfilePayloadState};

#[cfg(target_os = "linux")]
use crate::script_ui::{self, ScriptAction, ScriptUiState};
use crate::{
    bunnyhop::{self, BunnyhopUiState},
    floor_bounce::{self, FloorBounceUiState},
    freeze::{self, FreezeUiState},
    global_settings::{self, GlobalSettingsAction, GlobalSettingsState},
    hhj::{self, HhjGlobals, HhjUiState},
    item_clip::{self, ItemClipUiState},
    item_desync::{self, ItemDesyncUiState},
    item_unequip::{self, ItemUnequipGlobals, ItemUnequipUiState},
    keybind,
    lag_switch::{self, LagSwitchAction, LagSwitchUiState},
    laugh_clip::{self, LaughClipGlobals, LaughClipUiState},
    ledge_bounce::{self, LedgeBounceUiState},
    notification_suppression::{ADMIN_ELEVATION_WARNING_ID, NotificationSuppressionState},
    press_key::{self, PressKeyUiState},
    profile_browser::{self, ProfileAction, ProfileBrowserState},
    settings_menu, sidebar,
    spam_key::{self, SpamKeyUiState},
    speedglitch::{self, SpeedglitchUiState},
    theme,
    tutorial_media::TutorialMedia,
    wall_walk::{self, WallWalkUiState},
    walless_lhj::{self, WallessLhjGlobals, WallessLhjUiState},
    wallhop::{self, WallhopUiState},
    warning_toast::{self, WarningToastState},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShellAction {
    ToggleLagSwitchBackend,
    CheckUpdatesFromSettings,
    RequestInstallUpdateFromSettings,
    ConfirmInstallUpdate,
    SkipUpdate,
    CancelUpdate,
    OpenLatestReleaseFromSettings,
    OpenLatestReleaseAndDismiss,
    OpenSponsorFromSettings,
    OpenOfficialSource,
    Profile(ProfileAction),
    #[cfg(target_os = "linux")]
    Script(ScriptAction),
    #[cfg(target_os = "linux")]
    InstallLinuxInputPermissionsGraphical,
    #[cfg(target_os = "linux")]
    OpenLinuxInputPermissionTerminal,
    #[cfg(target_os = "linux")]
    CopyLinuxInputManualCommand,
    #[cfg(target_os = "linux")]
    OpenLinuxInputSetupDocs,
    #[cfg(target_os = "linux")]
    RetryLinuxInputPermissions,
    StartWaylandScreenCaptureFromSettings,
    StopWaylandScreenCaptureFromSettings,
    StartWaylandRemoteDesktopFromSettings,
    #[cfg(target_os = "linux")]
    StartLagSwitchHelper,
    PersistNotificationSuppression,
    #[cfg(target_os = "linux")]
    StartFreezeHelper,
    #[cfg(target_os = "linux")]
    CancelFreezeHelper,
    #[cfg(target_os = "linux")]
    ApproveWaylandScreenCapture,
    #[cfg(target_os = "linux")]
    DeclineWaylandScreenCapture,
    #[cfg(target_os = "linux")]
    ApproveWaylandRemoteDesktop,
    #[cfg(target_os = "linux")]
    DeclineWaylandRemoteDesktop,
}

const ROOT_WINDOW: &str = "Main SMU Window";
const GLOBAL_SETTINGS_CHILD: &str = "GlobalSettings";
const LEFT_SCROLL_CHILD: &str = "LeftScrollSection";
const RIGHT_SECTION_CHILD: &str = "RightSection";
const BOTTOM_CONTROLS_CHILD: &str = "BottomControls";

const SETTINGS_HORIZONTAL_MARGIN: f32 = 16.0;
const SETTINGS_TEXT_ROWS: f32 = 4.0;
const SETTINGS_PADDING_ROWS: f32 = 3.0;
const SETTINGS_EXTRA_HEIGHT: f32 = 12.0;
const LEFT_WIDTH_FRACTION: f32 = 0.30;
const MAIN_HORIZONTAL_ADJUSTMENT: f32 = 23.0;
const MAIN_VERTICAL_ADJUSTMENT: f32 = 20.0;
const FOOTER_HEIGHT: f32 = 30.0;
const FOOTER_BORDER_COMPENSATION: f32 = 1.0;
const FOOTER_CONTROLS_FROM_RIGHT: f32 = 602.0;
const FOOTER_CONTROL_Y_OFFSET: f32 = 3.0;
const OPACITY_SLIDER_WIDTH: f32 = 100.0;
const PROFILES_BUTTON_WIDTH: f32 = 125.0;

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum LinuxModal {
    LagSwitchHelper,
    FreezeHelper,
    WaylandScreenCapture,
    WaylandRemoteDesktop,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Default)]
struct LinuxModalState {
    pending_open: std::collections::BTreeSet<LinuxModal>,
    pending_lag_switch_helper_start: bool,
    admin_error: String,
}

#[derive(Debug)]
pub struct ShellState {
    always_on_top: bool,
    opacity_percent: f32,
    global: GlobalSettingsState,
    settings_open: bool,
    keybind_capture: keybind::CaptureState,
    settings_platform: settings_menu::PlatformStatus,
    settings_updater: settings_menu::UpdaterPanelStatus,
    notifications: NotificationSuppressionState,
    warning_toast: WarningToastState,
    theme: theme::ThemeState,
    tutorial_media: TutorialMedia,
    profile_browser: ProfileBrowserState,
    #[cfg(target_os = "linux")]
    script_ui: ScriptUiState,
    #[cfg(target_os = "linux")]
    linux_input_setup: crate::linux_input_setup::LinuxInputSetupStatus,
    platform_input_ready: bool,
    section_order: [usize; 16],
    selected_section: i32,
    freeze: FreezeUiState,
    floor_bounce: FloorBounceUiState,
    bunnyhop: BunnyhopUiState,
    hhj: HhjUiState,
    item_desync: ItemDesyncUiState,
    item_clip: ItemClipUiState,
    item_unequip: ItemUnequipUiState,
    laugh_clip: LaughClipUiState,
    press_key: PressKeyUiState,
    ledge_bounce: LedgeBounceUiState,
    spam_key: SpamKeyUiState,
    speedglitch: SpeedglitchUiState,
    wall_walk: WallWalkUiState,
    walless_lhj: WallessLhjUiState,
    wallhop: WallhopUiState,
    lag_switch: LagSwitchUiState,
    #[cfg(target_os = "linux")]
    linux_modals: LinuxModalState,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            always_on_top: false,
            opacity_percent: 100.0,
            global: GlobalSettingsState::default(),
            settings_open: false,
            keybind_capture: keybind::CaptureState::default(),
            settings_platform: settings_menu::PlatformStatus::default(),
            settings_updater: settings_menu::UpdaterPanelStatus::default(),
            notifications: NotificationSuppressionState::default(),
            warning_toast: WarningToastState::default(),
            theme: theme::ThemeState::default(),
            tutorial_media: TutorialMedia::default(),
            profile_browser: ProfileBrowserState::default(),
            #[cfg(target_os = "linux")]
            script_ui: ScriptUiState::default(),
            #[cfg(target_os = "linux")]
            linux_input_setup: crate::linux_input_setup::LinuxInputSetupStatus::default(),
            platform_input_ready: true,
            section_order: sidebar::DEFAULT_SECTION_ORDER,
            selected_section: -1,
            freeze: FreezeUiState::default(),
            floor_bounce: FloorBounceUiState::default(),
            bunnyhop: BunnyhopUiState::default(),
            hhj: HhjUiState::default(),
            item_desync: ItemDesyncUiState::default(),
            item_clip: ItemClipUiState::default(),
            item_unequip: ItemUnequipUiState::default(),
            laugh_clip: LaughClipUiState::default(),
            press_key: PressKeyUiState::default(),
            ledge_bounce: LedgeBounceUiState::default(),
            spam_key: SpamKeyUiState::default(),
            speedglitch: SpeedglitchUiState::default(),
            wall_walk: WallWalkUiState::default(),
            walless_lhj: WallessLhjUiState::default(),
            wallhop: WallhopUiState::default(),
            lag_switch: LagSwitchUiState::default(),
            #[cfg(target_os = "linux")]
            linux_modals: LinuxModalState::default(),
        }
    }
}

impl ShellState {
    #[must_use]
    pub fn for_parity_case(case: Option<&str>) -> Self {
        let mut state = Self::default();
        if case == Some("lag-switch-user-reference") {
            state.global.apply_lag_switch_reference_globals();
            state.selected_section = 15;
            state.freeze.enabled = false;
            state.lag_switch = LagSwitchUiState::user_reference();
        } else if case == Some("settings-user-reference") {
            state.settings_open = true;
        } else if case == Some("theme-editor-user-reference") {
            state.theme.toggle_editor();
        } else if case == Some("profile-menu-user-reference") {
            state.profile_browser.apply_parity_reference();
        } else if case == Some("sidebar-no-scroll-user-reference") {
            *state.global.short_descriptions_mut() = true;
        } else if case == Some("builtin-footer-user-reference") {
            state.selected_section = 0;
        } else if case == Some("import-preview-user-reference") {
            state.script_ui.apply_import_preview_parity_reference();
        } else if case == Some("import-preview-error-user-reference") {
            state.script_ui.apply_import_preview_error_parity_reference();
        } else if case == Some("warning-toast-user-reference") {
            state.warning_toast.push(warning_toast::PARITY_WARNING_MESSAGE);
        } else if case == Some("tutorial-media-user-reference") {
            state.selected_section = 9;
        }
        state
    }

    pub(crate) fn apply_profile_payload(&mut self, payload: &PreservedProfilePayload) {
        let enabled_before = self.section_enabled();
        let disable_before = self.section_disable_outside();

        self.global.apply_profile(payload);
        self.freeze.apply_profile(payload);
        self.item_desync.apply_profile(payload);
        self.hhj.apply_profile(payload);
        self.speedglitch.apply_profile(payload);
        self.item_unequip.apply_profile(payload);
        self.press_key.apply_profile(payload);
        self.wallhop.apply_profile(payload);
        self.walless_lhj.apply_profile(payload);
        self.item_clip.apply_profile(payload);
        self.laugh_clip.apply_profile(payload);
        self.wall_walk.apply_profile(payload);
        self.spam_key.apply_profile(payload);
        self.ledge_bounce.apply_profile(payload);
        self.bunnyhop.apply_profile(payload);
        self.floor_bounce.apply_profile(payload);
        self.lag_switch.apply_profile(payload);

        let mut enabled_after = enabled_before;
        let mut disable_after = disable_before;
        for index in 0..16 {
            if let Some(value) =
                crate::profile_adapter::section_flag(payload, "section_toggles", index)
            {
                enabled_after[index] = value;
            }
            if let Some(value) =
                crate::profile_adapter::section_flag(payload, "disable_outside_roblox", index)
            {
                disable_after[index] = value;
            }
        }
        if payload.raw_value("disable_outside_roblox").is_none() {
            if let Some(value) = crate::profile_adapter::bool_value(payload, "freezeoutsideroblox")
            {
                disable_after[0] = !value;
            }
            if let Some(value) = crate::profile_adapter::bool_value(payload, "unequipinroblox") {
                disable_after[4] = value;
            }
            if let Some(value) = crate::profile_adapter::bool_value(payload, "presskeyinroblox") {
                disable_after[5] = value;
            }
        }
        self.set_section_flags(enabled_after, disable_after);

        if let Some(value) = crate::profile_adapter::bool_value(payload, "ontoptoggle") {
            self.always_on_top = value;
        }
        if let Some(order) = payload.raw_value("section_order_vector").and_then(Value::as_array) {
            self.section_order = normalize_section_order(order, self.section_order);
        }

        if let Some(value) = crate::profile_adapter::i32_value(payload, "selected_section") {
            self.selected_section = value;
        }
        if let Some(value) = crate::profile_adapter::f32_value(payload, "windowOpacityPercent") {
            self.opacity_percent = value;
        }
    }

    pub(crate) fn reset_section_to_defaults(&mut self, section: usize) {
        if section >= 16 {
            return;
        }
        let defaults = PreservedProfilePayload::cpp_default();
        match section {
            0 => self.freeze.apply_profile(&defaults),
            1 => self.item_desync.apply_profile(&defaults),
            2 => self.hhj.apply_profile(&defaults),
            3 => self.speedglitch.apply_profile(&defaults),
            4 => {
                self.item_unequip.apply_profile(&defaults);
                if let Some(value) = crate::profile_adapter::u32_value(&defaults, "vk_enterkey") {
                    *self.global.enter_key_mut() = value;
                }
            }
            5 => self.press_key.reset_base_to_profile_defaults(&defaults),
            6 => self.wallhop.reset_base_to_profile_defaults(&defaults),
            7 => self.walless_lhj.apply_profile(&defaults),
            8 => self.item_clip.apply_profile(&defaults),
            9 => self.laugh_clip.apply_profile(&defaults),
            10 => self.wall_walk.apply_profile(&defaults),
            11 => self.spam_key.reset_base_to_profile_defaults(&defaults),
            12 => self.ledge_bounce.apply_profile(&defaults),
            13 => {
                self.bunnyhop.apply_profile(&defaults);
                if let Some(value) = crate::profile_adapter::bool_value(&defaults, "chatoverride") {
                    *self.global.chat_override_mut() = value;
                }
            }
            14 => self.floor_bounce.apply_profile(&defaults),
            15 => self.lag_switch.apply_profile(&defaults),
            _ => return,
        }

        // C++ ResetSectionToDefaults mutates the legacy flat section arrays for every section, but
        // the three multi-instance serializers source their visible base flags from instance[0]. The
        // resulting C++ payload therefore preserves those flags for sections 5/6/11; oracle fixtures
        // freeze that quirk and Rust intentionally mirrors the observable result.
        if !matches!(section, 5 | 6 | 11) {
            let mut enabled = self.section_enabled();
            let mut disable = self.section_disable_outside();
            if let Some(value) =
                crate::profile_adapter::section_flag(&defaults, "section_toggles", section)
            {
                enabled[section] = value;
            }
            if let Some(value) =
                crate::profile_adapter::section_flag(&defaults, "disable_outside_roblox", section)
            {
                disable[section] = value;
            }
            self.set_section_flags(enabled, disable);
        }
    }

    #[must_use]
    pub(crate) fn overlay_profile_payload(
        &self,
        baseline: &PreservedProfilePayload,
    ) -> Option<PreservedProfilePayload> {
        let mut overlay = Map::<String, Value>::new();
        self.global.overlay_profile(&mut overlay);
        self.freeze.overlay_profile(&mut overlay);
        self.item_desync.overlay_profile(&mut overlay);
        self.hhj.overlay_profile(&mut overlay);
        self.speedglitch.overlay_profile(&mut overlay);
        self.item_unequip.overlay_profile(&mut overlay);
        self.press_key.overlay_profile(&mut overlay);
        self.wallhop.overlay_profile(&mut overlay);
        self.walless_lhj.overlay_profile(&mut overlay);
        self.item_clip.overlay_profile(&mut overlay);
        self.laugh_clip.overlay_profile(&mut overlay);
        self.wall_walk.overlay_profile(&mut overlay);
        self.spam_key.overlay_profile(&mut overlay);
        self.ledge_bounce.overlay_profile(&mut overlay);
        self.bunnyhop.overlay_profile(&mut overlay);
        self.floor_bounce.overlay_profile(&mut overlay);
        self.lag_switch.overlay_profile(&mut overlay);

        crate::profile_adapter::insert_i32(&mut overlay, "selected_section", self.selected_section);
        crate::profile_adapter::insert_f32(
            &mut overlay,
            "windowOpacityPercent",
            self.opacity_percent,
        );
        crate::profile_adapter::insert_bool(&mut overlay, "ontoptoggle", self.always_on_top);
        overlay.insert(
            "section_order_vector".to_owned(),
            Value::Array(
                self.section_order
                    .iter()
                    .map(|value| Value::from(i64::try_from(*value).unwrap_or_default()))
                    .collect(),
            ),
        );

        let enabled = self.section_enabled();
        let disable = self.section_disable_outside();
        for index in 0..16 {
            overlay_section_state(
                baseline.normalized(),
                &mut overlay,
                index,
                enabled[index],
                disable[index],
            );
        }
        crate::profile_adapter::insert_bool(&mut overlay, "freezeoutsideroblox", !disable[0]);
        crate::profile_adapter::insert_bool(&mut overlay, "unequipinroblox", disable[4]);
        crate::profile_adapter::insert_bool(&mut overlay, "presskeyinroblox", disable[5]);
        let mut removals = Vec::new();
        if self.wallhop.instances().len() <= 1 {
            removals.push("extra_wallhop_instances");
        }
        if self.press_key.instances().len() <= 1 {
            removals.push("extra_presskey_instances");
        }
        if self.spam_key.instances().len() <= 1 {
            removals.push("extra_spamkey_instances");
        }
        baseline.with_overlay_and_removals(&overlay, &removals)
    }

    fn section_disable_outside(&self) -> [bool; 16] {
        let mut values = [false; 16];
        values[0] = self.freeze.disable_outside_target;
        values[1] = self.item_desync.disable_outside_target;
        values[2] = self.hhj.disable_outside_target;
        values[3] = self.speedglitch.disable_outside_target;
        values[4] = self.item_unequip.disable_outside_target;
        values[5] =
            self.press_key.instances().first().is_some_and(|value| value.disable_outside_target);
        values[6] =
            self.wallhop.instances().first().is_some_and(|value| value.disable_outside_target);
        values[7] = self.walless_lhj.disable_outside_target;
        values[8] = self.item_clip.disable_outside_target;
        values[9] = self.laugh_clip.disable_outside_target;
        values[10] = self.wall_walk.disable_outside_target;
        values[11] =
            self.spam_key.instances().first().is_some_and(|value| value.disable_outside_target);
        values[12] = self.ledge_bounce.disable_outside_target;
        values[13] = self.bunnyhop.disable_outside_target;
        values[14] = self.floor_bounce.disable_outside_target;
        values[15] = false;
        values
    }

    fn set_section_flags(&mut self, enabled: [bool; 16], disable: [bool; 16]) {
        self.freeze.enabled = enabled[0];
        self.freeze.disable_outside_target = disable[0];
        self.item_desync.enabled = enabled[1];
        self.item_desync.disable_outside_target = disable[1];
        self.hhj.enabled = enabled[2];
        self.hhj.disable_outside_target = disable[2];
        self.speedglitch.enabled = enabled[3];
        self.speedglitch.disable_outside_target = disable[3];
        self.item_unequip.enabled = enabled[4];
        self.item_unequip.disable_outside_target = disable[4];
        self.press_key.set_base_section_flags(enabled[5], disable[5]);
        self.wallhop.set_base_section_flags(enabled[6], disable[6]);
        self.walless_lhj.enabled = enabled[7];
        self.walless_lhj.disable_outside_target = disable[7];
        self.item_clip.enabled = enabled[8];
        self.item_clip.disable_outside_target = disable[8];
        self.laugh_clip.enabled = enabled[9];
        self.laugh_clip.disable_outside_target = disable[9];
        self.wall_walk.enabled = enabled[10];
        self.wall_walk.disable_outside_target = disable[10];
        self.spam_key.set_base_section_flags(enabled[11], disable[11]);
        self.ledge_bounce.enabled = enabled[12];
        self.ledge_bounce.disable_outside_target = disable[12];
        self.bunnyhop.enabled = enabled[13];
        self.bunnyhop.disable_outside_target = disable[13];
        self.floor_bounce.enabled = enabled[14];
        self.floor_bounce.disable_outside_target = disable[14];
        self.lag_switch.enabled = enabled[15];
    }

    #[must_use]
    pub const fn always_on_top(&self) -> bool {
        self.always_on_top
    }

    #[must_use]
    pub const fn opacity_percent(&self) -> f32 {
        self.opacity_percent
    }

    #[must_use]
    const fn macro_runtime_enabled(&self) -> bool {
        self.global.macro_enabled()
            && self.keybind_capture.runtime_allowed()
            && self.platform_input_ready
    }

    #[must_use]
    pub const fn keybind_input_poll_needed(&self) -> bool {
        self.keybind_capture.input_poll_needed()
    }

    #[must_use]
    pub const fn keybind_capture_active(&self) -> bool {
        self.keybind_capture.capture_active()
    }

    #[must_use]
    pub const fn input_capture_active(&self) -> bool {
        self.keybind_capture.input_capture_active()
    }

    pub const fn set_external_input_capture(&mut self, active: bool) {
        self.keybind_capture.set_external_capture(active);
    }

    pub const fn set_keybind_input(&mut self, input: keybind::InputSnapshot) {
        self.keybind_capture.set_input(input);
    }

    #[cfg(target_os = "linux")]
    pub fn set_settings_platform_status(&mut self, status: settings_menu::PlatformStatus) {
        self.settings_platform = status;
    }

    #[cfg(target_os = "linux")]
    pub fn set_linux_input_setup_status(
        &mut self,
        status: crate::linux_input_setup::LinuxInputSetupStatus,
    ) {
        self.platform_input_ready = !status.required;
        self.linux_input_setup = status;
    }

    #[cfg(target_os = "linux")]
    pub fn set_settings_updater_status(&mut self, status: settings_menu::UpdaterPanelStatus) {
        self.settings_updater = status;
    }

    pub fn push_warning(&mut self, message: impl Into<String>) {
        self.warning_toast.push(message);
    }

    pub const fn set_tutorial_media(&mut self, media: TutorialMedia) {
        self.tutorial_media = media;
    }

    pub const fn profile_browser_mut(&mut self) -> &mut ProfileBrowserState {
        &mut self.profile_browser
    }

    pub fn apply_profile_metadata(&mut self, metadata: &Map<String, Value>) {
        self.global.apply_metadata(metadata);
        self.notifications.apply_metadata(metadata);
        self.theme.apply_metadata(metadata);
    }

    #[must_use]
    pub fn profile_metadata_overlay(&self) -> Map<String, Value> {
        let mut metadata = self.global.metadata_overlay();
        metadata.extend(self.notifications.metadata_overlay());
        metadata.extend(self.theme.metadata_overlay());
        metadata
    }

    #[must_use]
    pub fn admin_warning_suppressed(&self) -> bool {
        self.notifications.is_suppressed(ADMIN_ELEVATION_WARNING_ID)
    }

    pub fn set_admin_warning_suppressed(&mut self, suppressed: bool) -> bool {
        self.notifications.set_suppressed(ADMIN_ELEVATION_WARNING_ID, suppressed)
    }

    pub fn apply_theme_to_context(&self, context: &mut dear_imgui_rs::Context) {
        self.theme.apply_to_context(context);
    }

    #[cfg(target_os = "linux")]
    pub const fn script_ui_mut(&mut self) -> &mut ScriptUiState {
        &mut self.script_ui
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub const fn selected_script(&self) -> Option<usize> {
        self.script_ui.selected()
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn process_selector(&self) -> String {
        self.global.process_selector()
    }

    #[cfg(target_os = "linux")]
    pub const fn set_process_found(&mut self, found: bool) {
        self.global.set_process_found(found);
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn script_runtime_settings(&self) -> crate::script_runtime::ScriptRuntimeSettings {
        crate::script_runtime::ScriptRuntimeSettings {
            macro_enabled: self.macro_runtime_enabled(),
            process_selector: self.global.process_selector(),
        }
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn script_process_target(&self) -> (String, bool) {
        (self.global.process_selector(), self.freeze.take_all_process_ids)
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn freeze_controller_settings(&self) -> crate::freeze_runtime::FreezeControllerSettings {
        self.freeze
            .controller_settings(self.macro_runtime_enabled(), self.global.process_selector())
    }

    #[must_use]
    fn section_enabled(&self) -> [bool; 16] {
        let mut enabled = [false; 16];
        enabled[0] = self.freeze.enabled;
        enabled[1] = self.item_desync.enabled;
        enabled[2] = self.hhj.enabled;
        enabled[3] = self.speedglitch.enabled;
        enabled[4] = self.item_unequip.enabled;
        enabled[5] = self.press_key.first_enabled();
        enabled[6] = self.wallhop.first_enabled();
        enabled[7] = self.walless_lhj.enabled;
        enabled[8] = self.item_clip.enabled;
        enabled[9] = self.laugh_clip.enabled;
        enabled[10] = self.wall_walk.enabled;
        enabled[11] = self.spam_key.first_enabled();
        enabled[12] = self.ledge_bounce.enabled;
        enabled[13] = self.bunnyhop.enabled;
        enabled[14] = self.floor_bounce.enabled;
        enabled[15] = self.lag_switch.enabled;
        enabled
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn floor_bounce_controller_settings(
        &self,
    ) -> crate::floor_bounce_runtime::FloorBounceControllerSettings {
        self.floor_bounce.controller_settings(
            self.macro_runtime_enabled(),
            self.global.process_selector(),
            self.freeze.take_all_process_ids,
            self.global.shift_key(),
        )
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn hhj_controller_settings(&self) -> crate::hhj_runtime::HhjControllerSettings {
        let (strength_x, strength_y) = self.speedglitch.motion_strengths();
        self.hhj.controller_settings(HhjGlobals {
            macro_enabled: self.macro_runtime_enabled(),
            process_selector: self.global.process_selector(),
            take_all_process_ids: self.freeze.take_all_process_ids,
            global_zoom: self.global.global_zoom(),
            shift_key: self.global.shift_key(),
            zoom_reverse: self.global.zoom_reverse(),
            fps: self.global.fps_runtime(),
            strength_x,
            strength_y,
        })
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn item_unequip_controller_settings(
        &self,
    ) -> crate::item_unequip_runtime::ItemUnequipControllerSettings {
        self.item_unequip.controller_settings(ItemUnequipGlobals {
            macro_enabled: self.macro_runtime_enabled(),
            process_selector: self.global.process_selector(),
            chat_override: self.global.chat_override(),
            chat_key: self.global.chat_key(),
            enter_key: self.global.enter_key(),
            paste_delay_ms: self.global.paste_delay_ms(),
        })
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn item_desync_controller_settings(
        &self,
    ) -> crate::item_desync_runtime::ItemDesyncControllerSettings {
        self.item_desync
            .controller_settings(self.macro_runtime_enabled(), self.global.process_selector())
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn item_clip_controller_settings(
        &self,
    ) -> crate::item_clip_runtime::ItemClipControllerSettings {
        self.item_clip
            .controller_settings(self.macro_runtime_enabled(), self.global.process_selector())
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn laugh_clip_controller_settings(
        &self,
    ) -> crate::laugh_clip_runtime::LaughClipControllerSettings {
        self.laugh_clip.controller_settings(LaughClipGlobals {
            macro_enabled: self.macro_runtime_enabled(),
            process_selector: self.global.process_selector(),
            chat_override: self.global.chat_override(),
            chat_key: self.global.chat_key(),
            enter_key: self.global.enter_key(),
            paste_delay_ms: self.global.paste_delay_ms(),
            global_zoom: self.global.global_zoom(),
            shift_key: self.global.shift_key(),
            zoom_reverse: self.global.zoom_reverse(),
        })
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn spam_key_controller_settings(
        &self,
    ) -> crate::spam_key_runtime::SpamKeyControllerSettings {
        self.spam_key
            .controller_settings(self.macro_runtime_enabled(), self.global.process_selector())
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn bunnyhop_controller_settings(
        &self,
    ) -> crate::bunnyhop_runtime::BunnyhopControllerSettings {
        self.bunnyhop.controller_settings(
            self.macro_runtime_enabled(),
            self.global.chat_key(),
            self.global.process_selector(),
        )
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn ledge_bounce_controller_settings(
        &self,
    ) -> crate::ledge_bounce_runtime::LedgeBounceControllerSettings {
        self.ledge_bounce.controller_settings(
            self.macro_runtime_enabled(),
            self.global.sensitivity().to_owned(),
            self.global.cam_fix_enabled(),
            self.global.process_selector(),
        )
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn speedglitch_controller_settings(
        &self,
    ) -> crate::speedglitch_runtime::SpeedglitchControllerSettings {
        self.speedglitch.controller_settings(
            self.macro_runtime_enabled(),
            self.global.fps_runtime(),
            self.global.process_selector(),
        )
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn press_key_controller_settings(
        &self,
    ) -> crate::press_key_runtime::PressKeyControllerSettings {
        self.press_key
            .controller_settings(self.macro_runtime_enabled(), self.global.process_selector())
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn wallhop_controller_settings(&self) -> crate::wallhop_runtime::WallhopControllerSettings {
        self.wallhop
            .controller_settings(self.macro_runtime_enabled(), self.global.process_selector())
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn walless_lhj_controller_settings(
        &self,
    ) -> crate::walless_lhj_runtime::WallessLhjControllerSettings {
        self.walless_lhj.controller_settings(WallessLhjGlobals {
            macro_enabled: self.macro_runtime_enabled(),
            process_selector: self.global.process_selector(),
            global_zoom: self.global.global_zoom(),
            shift_key: self.global.shift_key(),
            zoom_reverse: self.global.zoom_reverse(),
            take_all_process_ids: self.freeze.take_all_process_ids,
        })
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn wall_walk_controller_settings(
        &self,
    ) -> crate::wall_walk_runtime::WallWalkControllerSettings {
        self.wall_walk.controller_settings(
            self.macro_runtime_enabled(),
            self.global.fps_runtime(),
            self.global.process_selector(),
        )
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn lag_switch_controller_settings(
        &self,
    ) -> crate::lag_switch_runtime::LagSwitchControllerSettings {
        self.lag_switch.controller_settings(self.macro_runtime_enabled())
    }

    #[cfg(target_os = "linux")]
    pub const fn set_lag_switch_backend(&mut self, backend: crate::lag_switch::BackendState) {
        self.lag_switch.backend = backend;
    }

    #[cfg(target_os = "linux")]
    pub fn request_lag_switch_helper_popup(&mut self) {
        if self.admin_warning_suppressed() {
            self.linux_modals.pending_lag_switch_helper_start = true;
        } else {
            self.linux_modals.pending_open.insert(LinuxModal::LagSwitchHelper);
        }
    }

    #[cfg(target_os = "linux")]
    pub fn set_lag_switch_helper_error(&mut self, error: impl Into<String>) {
        self.linux_modals.admin_error = error.into();
    }

    #[cfg(target_os = "linux")]
    pub fn request_freeze_helper_popup(&mut self) {
        self.linux_modals.pending_open.insert(LinuxModal::FreezeHelper);
    }

    #[cfg(target_os = "linux")]
    pub fn request_wayland_screen_capture_popup(&mut self) {
        self.linux_modals.pending_open.insert(LinuxModal::WaylandScreenCapture);
    }

    #[cfg(target_os = "linux")]
    pub fn request_wayland_remote_desktop_popup(&mut self) {
        self.linux_modals.pending_open.insert(LinuxModal::WaylandRemoteDesktop);
    }
}

fn normalize_section_order(values: &[Value], fallback: [usize; 16]) -> [usize; 16] {
    let mut order = values
        .iter()
        .filter_map(Value::as_i64)
        .filter_map(|value| usize::try_from(value).ok())
        .collect::<Vec<_>>();
    if !order.contains(&13) && order.len() >= 7 {
        order.insert(7, 13);
    }
    if !order.contains(&15) && order.len() >= 5 {
        order.insert(4, 15);
    }
    let mut final_order = Vec::with_capacity(16);
    for section in order {
        if section < 16 && !final_order.contains(&section) {
            final_order.push(section);
        }
    }
    for section in 0..16 {
        if !final_order.contains(&section) {
            final_order.push(section);
        }
    }
    <[usize; 16]>::try_from(final_order).unwrap_or(fallback)
}

fn overlay_section_state(
    baseline: &ProfilePayloadState,
    overlay: &mut Map<String, Value>,
    index: usize,
    enabled: bool,
    disable_outside: bool,
) {
    crate::profile_adapter::overlay_section_flag(
        baseline,
        overlay,
        "section_toggles",
        index,
        enabled,
    );
    crate::profile_adapter::overlay_section_flag(
        baseline,
        overlay,
        "disable_outside_roblox",
        index,
        disable_outside,
    );
}

pub fn render(ui: &Ui, state: &mut ShellState) -> Option<ShellAction> {
    let (display_size, active_theme) = (ui.io().display_size(), state.theme.active().clone());
    let flags = WindowFlags::NO_RESIZE
        | WindowFlags::NO_MOVE
        | WindowFlags::NO_TITLE_BAR
        | WindowFlags::NO_SCROLLBAR
        | WindowFlags::NO_SCROLL_WITH_MOUSE
        | WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS;

    let mut action = None;
    let _window_rounding = ui.push_style_var(StyleVar::WindowRounding(0.0));
    ui.window(ROOT_WINDOW)
        .size(display_size, Condition::Always)
        .position([0.0, 0.0], Condition::Always)
        .flags(flags)
        .build(|| {
            action = render_contents(ui, state, display_size, &active_theme);
        });

    warning_toast::render(ui, &mut state.warning_toast);

    #[cfg(target_os = "linux")]
    if let Some(modal_action) = render_linux_modals(ui, state, &active_theme) {
        action = Some(modal_action);
    }
    #[cfg(target_os = "linux")]
    if let Some(setup_action) = render_linux_input_setup(ui, state, &active_theme) {
        action = Some(setup_action);
    }

    #[cfg(target_os = "linux")]
    if let Some(script_action) =
        script_ui::render_trust_modal(ui, &mut state.script_ui, &active_theme)
    {
        action = Some(ShellAction::Script(script_action));
    }

    if let Some(settings_action) = settings_menu::render(
        ui,
        &mut state.settings_open,
        &mut state.global,
        &mut state.keybind_capture,
        &state.settings_platform,
        &state.settings_updater,
        &active_theme,
    ) {
        action = Some(map_settings_action(settings_action));
    }

    if let Some(update_action) =
        settings_menu::render_update_confirmation(ui, &state.settings_updater)
    {
        action = Some(map_settings_action(update_action));
    }

    #[cfg(target_os = "linux")]
    take_pending_lag_switch_helper_start(state, &mut action);
    state.theme.render_editor(ui);
    action
}

const fn map_settings_action(action: settings_menu::SettingsAction) -> ShellAction {
    match action {
        settings_menu::SettingsAction::CheckUpdates => ShellAction::CheckUpdatesFromSettings,
        settings_menu::SettingsAction::RequestInstallUpdate => {
            ShellAction::RequestInstallUpdateFromSettings
        }
        settings_menu::SettingsAction::ConfirmInstallUpdate => ShellAction::ConfirmInstallUpdate,
        settings_menu::SettingsAction::SkipUpdate => ShellAction::SkipUpdate,
        settings_menu::SettingsAction::CancelUpdate => ShellAction::CancelUpdate,
        settings_menu::SettingsAction::OpenLatestRelease => {
            ShellAction::OpenLatestReleaseFromSettings
        }
        settings_menu::SettingsAction::OpenLatestReleaseAndDismiss => {
            ShellAction::OpenLatestReleaseAndDismiss
        }
        settings_menu::SettingsAction::OpenSponsor => ShellAction::OpenSponsorFromSettings,
        settings_menu::SettingsAction::StartWaylandScreenCapture => {
            ShellAction::StartWaylandScreenCaptureFromSettings
        }
        settings_menu::SettingsAction::StopWaylandScreenCapture => {
            ShellAction::StopWaylandScreenCaptureFromSettings
        }
        settings_menu::SettingsAction::StartWaylandRemoteDesktop => {
            ShellAction::StartWaylandRemoteDesktopFromSettings
        }
    }
}

#[cfg(target_os = "linux")]
fn take_pending_lag_switch_helper_start(state: &mut ShellState, action: &mut Option<ShellAction>) {
    if state.linux_modals.pending_lag_switch_helper_start {
        state.linux_modals.pending_lag_switch_helper_start = false;
        *action = Some(ShellAction::StartLagSwitchHelper);
    }
}

#[cfg(target_os = "linux")]
fn render_linux_input_setup(
    ui: &Ui,
    state: &ShellState,
    theme: &theme::ThemeDefinition,
) -> Option<ShellAction> {
    let action = crate::linux_input_setup::render(ui, &state.linux_input_setup, theme)?;
    Some(match action {
        crate::linux_input_setup::LinuxInputSetupAction::InstallGraphical => {
            ShellAction::InstallLinuxInputPermissionsGraphical
        }
        crate::linux_input_setup::LinuxInputSetupAction::OpenTerminalInstaller => {
            ShellAction::OpenLinuxInputPermissionTerminal
        }
        crate::linux_input_setup::LinuxInputSetupAction::CopyManualCommand => {
            ShellAction::CopyLinuxInputManualCommand
        }
        crate::linux_input_setup::LinuxInputSetupAction::OpenSetupDocs => {
            ShellAction::OpenLinuxInputSetupDocs
        }
        crate::linux_input_setup::LinuxInputSetupAction::Retry => {
            ShellAction::RetryLinuxInputPermissions
        }
    })
}

#[cfg(target_os = "linux")]
fn open_linux_modal(ui: &Ui, state: &mut LinuxModalState, modal: LinuxModal, name: &str) {
    if state.pending_open.remove(&modal) {
        let _ = ui.open_popup(name);
    }
}

#[cfg(target_os = "linux")]
fn render_linux_modals(
    ui: &Ui,
    state: &mut ShellState,
    theme: &theme::ThemeDefinition,
) -> Option<ShellAction> {
    if let Some(action) = render_lag_switch_helper_modal(ui, state, theme) {
        return Some(action);
    }
    if let Some(action) = render_freeze_helper_modal(ui, &mut state.linux_modals) {
        return Some(action);
    }
    if let Some(action) = render_wayland_screen_capture_modal(ui, &mut state.linux_modals) {
        return Some(action);
    }
    render_wayland_remote_desktop_modal(ui, &mut state.linux_modals)
}

#[cfg(target_os = "linux")]
fn render_lag_switch_helper_modal(
    ui: &Ui,
    state: &mut ShellState,
    theme: &theme::ThemeDefinition,
) -> Option<ShellAction> {
    open_linux_modal(
        ui,
        &mut state.linux_modals,
        LinuxModal::LagSwitchHelper,
        "Administrator Required",
    );
    let _popup = ui.begin_modal_popup("Administrator Required")?;
    ui.text_wrapped("Linux lagswitch features require a privileged nethelper process.");
    ui.text_wrapped("This process involves:");
    ui.bullet_text("Starting the bundled nethelper with pkexec.");
    ui.bullet_text("Keeping the SMU GUI running as your normal desktop user.");
    if !state.linux_modals.admin_error.is_empty() {
        ui.text_colored(theme.error_color, &state.linux_modals.admin_error);
    }
    crate::parity::separator(ui);
    let mut action = None;
    let mut suppress_admin_warning = state.admin_warning_suppressed();
    if ui.checkbox("Do not show this again", &mut suppress_admin_warning) {
        let _ = state.set_admin_warning_suppressed(suppress_admin_warning);
        action = Some(ShellAction::PersistNotificationSuppression);
    }
    crate::parity::separator(ui);
    if ui.button_with_size("Start Helper", [180.0, 0.0]) {
        ui.close_current_popup();
        return Some(ShellAction::StartLagSwitchHelper);
    }
    ui.same_line();
    if ui.button_with_size("Cancel", [120.0, 0.0]) {
        ui.close_current_popup();
    }
    action
}

#[cfg(target_os = "linux")]
fn render_freeze_helper_modal(ui: &Ui, state: &mut LinuxModalState) -> Option<ShellAction> {
    open_linux_modal(ui, state, LinuxModal::FreezeHelper, "Additional Freeze Permission Required");
    let _popup = ui.begin_modal_popup("Additional Freeze Permission Required")?;
    ui.text_wrapped(
        "SMU cannot move Sober's game process into the temporary cgroup used by Freeze with your current Linux permissions.",
    );
    ui.new_line();
    ui.text_wrapped(
        "To keep Sober-compatible freezing working, SMU can start its bundled privileged Linux helper with pkexec.",
    );
    ui.text_wrapped("This grants the helper root access for this SMU session.");
    ui.new_line();
    ui.bullet_text("The SMU GUI remains running as your normal desktop user.");
    ui.bullet_text("The helper only accepts commands from this exact SMU process.");
    ui.bullet_text("The helper exits when SMU exits; nothing is installed permanently.");
    ui.bullet_text(
        "This helper is bundled with SMU and is dedicated to the Sober freeze workaround.",
    );
    crate::parity::separator(ui);
    if ui.button_with_size("Start Privileged Helper", [220.0, 0.0]) {
        ui.close_current_popup();
        return Some(ShellAction::StartFreezeHelper);
    }
    ui.same_line();
    if ui.button_with_size("Cancel##FreezeHelper", [120.0, 0.0]) {
        ui.close_current_popup();
        return Some(ShellAction::CancelFreezeHelper);
    }
    None
}

#[cfg(target_os = "linux")]
fn render_wayland_screen_capture_modal(
    ui: &Ui,
    state: &mut LinuxModalState,
) -> Option<ShellAction> {
    open_linux_modal(ui, state, LinuxModal::WaylandScreenCapture, "Enable Wayland Screen Capture?");
    let _popup = ui.begin_modal_popup("Enable Wayland Screen Capture?")?;
    ui.text_wrapped(
        "A running Lua script called getPixelColor() or getPixelRect(). On Wayland, SMU must ask the desktop portal for permission to capture one monitor.",
    );
    ui.text_wrapped(
        "Enable will open your desktop's monitor picker. The script stays paused until a monitor is selected and the first frame is ready.",
    );
    crate::parity::separator(ui);
    if ui.button("Enable and Select Monitor") {
        ui.close_current_popup();
        return Some(ShellAction::ApproveWaylandScreenCapture);
    }
    ui.same_line();
    if ui.button("Don't Enable##WaylandScreenCapture") {
        ui.close_current_popup();
        return Some(ShellAction::DeclineWaylandScreenCapture);
    }
    None
}

#[cfg(target_os = "linux")]
fn render_wayland_remote_desktop_modal(
    ui: &Ui,
    state: &mut LinuxModalState,
) -> Option<ShellAction> {
    open_linux_modal(
        ui,
        state,
        LinuxModal::WaylandRemoteDesktop,
        "Enable Wayland Absolute Mouse Control?",
    );
    let _popup = ui.begin_modal_popup("Enable Wayland Absolute Mouse Control?")?;
    ui.text_wrapped(
        "A running Lua script called moveMouseAbs(). On Wayland, SMU needs explicit permission to control the pointer and capture one monitor through the desktop portal.",
    );
    ui.text_wrapped(
        "Enable opens your desktop's permission dialog. The script remains paused until pointer control and the selected monitor's first frame are ready. This does not let SMU read the current global cursor position.",
    );
    crate::parity::separator(ui);
    if ui.button("Enable Pointer Control and Select Monitor") {
        ui.close_current_popup();
        return Some(ShellAction::ApproveWaylandRemoteDesktop);
    }
    ui.same_line();
    if ui.button("Don't Enable##WaylandRemoteDesktop") {
        ui.close_current_popup();
        return Some(ShellAction::DeclineWaylandRemoteDesktop);
    }
    None
}

fn render_macro_sidebar(
    ui: &Ui,
    state: &mut ShellState,
    left_panel_width: f32,
    left_panel_height: f32,
    theme: &theme::ThemeDefinition,
) -> Option<ShellAction> {
    let delta_time = ui.io().delta_time();
    state.press_key.tick_remove_confirmation(delta_time, state.selected_section == 5);
    state.wallhop.tick_remove_confirmation(delta_time, state.selected_section == 6);
    state.spam_key.tick_remove_confirmation(delta_time, state.selected_section == 11);
    let section_enabled = state.section_enabled();
    let multi_instances = [
        sidebar::MultiInstanceSummary {
            section_index: 5,
            enabled: state.press_key.enabled_flags(),
            selected_instance: state.press_key.selected_index(),
        },
        sidebar::MultiInstanceSummary {
            section_index: 6,
            enabled: state.wallhop.enabled_flags(),
            selected_instance: state.wallhop.selected_index(),
        },
        sidebar::MultiInstanceSummary {
            section_index: 11,
            enabled: state.spam_key.enabled_flags(),
            selected_instance: state.spam_key.selected_index(),
        },
    ];
    let mut sidebar_action = None;
    #[cfg(target_os = "linux")]
    let mut script_action = None;
    ui.child_window(LEFT_SCROLL_CHILD)
        .size([left_panel_width, left_panel_height])
        .border(true)
        .build(ui, || {
            sidebar_action = sidebar::render(
                ui,
                left_panel_width,
                &mut state.selected_section,
                sidebar::SidebarRenderConfig {
                    section_order: &state.section_order,
                    section_enabled: &section_enabled,
                    multi_instances: &multi_instances,
                    short_descriptions: state.global.short_descriptions(),
                    theme,
                },
            );
            #[cfg(target_os = "linux")]
            {
                script_action =
                    script_ui::render_sidebar(ui, &mut state.script_ui, left_panel_width, theme);
            }
        });
    if sidebar_action.is_some() {
        #[cfg(target_os = "linux")]
        state.script_ui.clear_selection();
    }
    match sidebar_action {
        Some(sidebar::SidebarAction::SelectInstance { section_index: 5, instance_index }) => {
            state.press_key.select(instance_index);
        }
        Some(sidebar::SidebarAction::AddInstance { section_index: 5 }) => {
            state.press_key.add_default();
        }
        Some(sidebar::SidebarAction::SelectInstance { section_index: 6, instance_index }) => {
            state.wallhop.select(instance_index);
        }
        Some(sidebar::SidebarAction::AddInstance { section_index: 6 }) => {
            state.wallhop.add_default();
        }
        Some(sidebar::SidebarAction::SelectInstance { section_index: 11, instance_index }) => {
            state.spam_key.select(instance_index);
        }
        Some(sidebar::SidebarAction::AddInstance { section_index: 11 }) => {
            state.spam_key.add_default();
        }
        _ => {}
    }
    #[cfg(target_os = "linux")]
    if let Some(action) = script_action {
        return Some(ShellAction::Script(action));
    }
    None
}

#[allow(clippy::suboptimal_flops)] // Preserve the C++ oracle's expression order for pixel parity.
fn render_contents(
    ui: &Ui,
    state: &mut ShellState,
    display_size: [f32; 2],
    theme: &theme::ThemeDefinition,
) -> Option<ShellAction> {
    let style = ui.clone_style();
    let settings_panel_height = ui.text_line_height_with_spacing() * SETTINGS_TEXT_ROWS
        + style.window_padding()[1] * SETTINGS_PADDING_ROWS
        + SETTINGS_EXTRA_HEIGHT;

    let mut global_action = None;
    let settings_open = state.settings_open;
    let theme_editor_open = state.theme.editor_open();
    ui.child_window(GLOBAL_SETTINGS_CHILD)
        .size([display_size[0] - SETTINGS_HORIZONTAL_MARGIN, settings_panel_height])
        .border(true)
        .build(ui, || {
            global_action = global_settings::render(
                ui,
                &mut state.global,
                state.settings_updater.user_outdated(),
                settings_open,
                theme_editor_open,
                theme,
            );
        });
    let global_shell_action = match global_action {
        Some(GlobalSettingsAction::ToggleSettings) => {
            state.settings_open = !state.settings_open;
            None
        }
        Some(GlobalSettingsAction::ToggleThemeEditor) => {
            state.theme.toggle_editor();
            None
        }
        Some(GlobalSettingsAction::OpenOfficialSource) => Some(ShellAction::OpenOfficialSource),
        None => None,
    };
    state.speedglitch.sync_global_calibration(
        state.global.sensitivity(),
        state.global.cam_fix_enabled(),
        state.global.speed_calibration_revision(),
    );
    state.wall_walk.sync_global_calibration(
        state.global.sensitivity(),
        state.global.cam_fix_enabled(),
        state.global.speed_calibration_revision(),
    );
    state.wallhop.sync_sensitivity_edit(
        state.global.sensitivity(),
        state.global.cam_fix_enabled(),
        state.global.wallhop_sensitivity_revision(),
    );

    let left_panel_width = ui.window_size()[0] * LEFT_WIDTH_FRACTION - MAIN_HORIZONTAL_ADJUSTMENT;
    let left_panel_height = ui.window_size()[1] - settings_panel_height - MAIN_VERTICAL_ADJUSTMENT;
    let sidebar_shell_action =
        render_macro_sidebar(ui, state, left_panel_width, left_panel_height, theme);

    ui.same_line();
    let right_section_size = [
        display_size[0] - MAIN_HORIZONTAL_ADJUSTMENT - left_panel_width,
        display_size[1] - settings_panel_height - MAIN_VERTICAL_ADJUSTMENT - FOOTER_HEIGHT,
    ];
    let (lag_action, script_pane_action) =
        render_right_section(ui, state, right_section_size, theme);

    let right_child_min = ui.item_rect_min();
    let right_child_size = ui.item_rect_size();
    let window_size = ui.window_size();
    ui.set_cursor_pos_y(window_size[1] - FOOTER_HEIGHT - style.window_padding()[1]);
    ui.set_cursor_pos_x(right_child_min[0]);

    let _square_footer = ui.push_style_var(StyleVar::ChildRounding(0.0));
    let mut footer_action = None;
    ui.child_window(BOTTOM_CONTROLS_CHILD)
        .size([right_child_size[0] - FOOTER_BORDER_COMPENSATION, FOOTER_HEIGHT])
        .flags(WindowFlags::NO_SCROLLBAR | WindowFlags::NO_SCROLL_WITH_MOUSE)
        .build(ui, || {
            footer_action = render_footer(ui, right_child_size[0], state, theme);
        });

    global_shell_action
        .or(sidebar_shell_action)
        .or(script_pane_action)
        .or_else(|| {
            lag_action.map(|LagSwitchAction::ToggleBackend| ShellAction::ToggleLagSwitchBackend)
        })
        .or(footer_action)
}

fn render_right_section(
    ui: &Ui,
    state: &mut ShellState,
    size: [f32; 2],
    theme: &theme::ThemeDefinition,
) -> (Option<LagSwitchAction>, Option<ShellAction>) {
    let mut lag_action = None;
    let mut script_action = None;
    ui.child_window(RIGHT_SECTION_CHILD).size(size).border(true).build(ui, || {
        #[cfg(target_os = "linux")]
        if state.script_ui.selected().is_some() {
            script_action =
                script_ui::render_selected(ui, &state.script_ui, &mut state.keybind_capture, theme)
                    .map(ShellAction::Script);
            return;
        }
        match state.selected_section {
            0 => freeze::render(ui, &mut state.freeze, &mut state.keybind_capture, theme),
            1 => item_desync::render(ui, &mut state.item_desync, &mut state.keybind_capture, theme),
            2 => hhj::render(ui, &mut state.hhj, &mut state.keybind_capture, theme),
            3 => speedglitch::render(ui, &mut state.speedglitch, &mut state.keybind_capture, theme),
            4 => item_unequip::render(
                ui,
                &mut state.item_unequip,
                state.global.enter_key_mut(),
                &mut state.keybind_capture,
                theme,
            ),
            5 => press_key::render(ui, &mut state.press_key, &mut state.keybind_capture, theme),
            6 => wallhop::render(
                ui,
                &mut state.wallhop,
                state.global.sensitivity(),
                state.global.cam_fix_enabled(),
                &mut state.keybind_capture,
                theme,
                state.tutorial_media,
            ),
            7 => walless_lhj::render(ui, &mut state.walless_lhj, &mut state.keybind_capture, theme),
            8 => item_clip::render(
                ui,
                &mut state.item_clip,
                &mut state.keybind_capture,
                theme,
                state.tutorial_media,
            ),
            9 => laugh_clip::render(
                ui,
                &mut state.laugh_clip,
                &mut state.keybind_capture,
                theme,
                state.tutorial_media,
            ),
            10 => wall_walk::render(
                ui,
                &mut state.wall_walk,
                state.global.sensitivity(),
                state.global.cam_fix_enabled(),
                &mut state.keybind_capture,
                theme,
                state.tutorial_media,
            ),
            11 => spam_key::render(
                ui,
                &mut state.spam_key,
                &mut state.keybind_capture,
                theme,
                state.tutorial_media,
            ),
            12 => {
                ledge_bounce::render(
                    ui,
                    &mut state.ledge_bounce,
                    &mut state.keybind_capture,
                    theme,
                );
            }
            13 => bunnyhop::render(ui, &mut state.bunnyhop, &mut state.keybind_capture, theme),
            14 => {
                floor_bounce::render(
                    ui,
                    &mut state.floor_bounce,
                    &mut state.keybind_capture,
                    theme,
                );
            }
            15 => {
                lag_action = lag_switch::render(
                    ui,
                    &mut state.lag_switch,
                    &mut state.keybind_capture,
                    theme,
                );
            }
            _ => ui.text_wrapped("Select a section to see its settings."),
        }
    });
    (lag_action, script_action)
}

#[allow(clippy::suboptimal_flops)] // Preserve the C++ oracle's expression order for pixel parity.
fn render_footer(
    ui: &Ui,
    right_child_width: f32,
    state: &mut ShellState,
    theme: &theme::ThemeDefinition,
) -> Option<ShellAction> {
    ui.same_line_with_pos((right_child_width - FOOTER_CONTROLS_FROM_RIGHT).max(0.0));
    ui.align_text_to_frame_padding();
    nudge_footer_y(ui);
    ui.text("Always On-Top");
    ui.same_line();
    nudge_footer_y(ui);
    ui.checkbox("##OnTopToggle", &mut state.always_on_top);
    ui.same_line();
    nudge_footer_y(ui);
    ui.text("Opacity");
    ui.same_line();
    ui.set_next_item_width(OPACITY_SLIDER_WIDTH);
    nudge_footer_y(ui);
    if let Ok(slider) =
        ui.slider_config("##OpacitySlider", 20.0_f32, 100.0_f32).try_display_format("%.0f%%")
    {
        slider.build(&mut state.opacity_percent);
    }

    let style = ui.clone_style();
    let revert_width = ui.calc_text_size("Revert Module")[0] + style.frame_padding()[0] * 2.0;
    ui.same_line_with_pos(
        right_child_width - PROFILES_BUTTON_WIDTH - style.item_spacing()[0] - revert_width,
    );
    nudge_footer_y(ui);

    let mut footer_action = None;
    #[cfg(target_os = "linux")]
    if let Some(script) = state.script_ui.selected_snapshot().cloned() {
        let disabled = script.running.then(|| ui.begin_disabled());
        if ui.button("Revert Module")
            && let Some(index) = state.script_ui.selected()
        {
            footer_action = Some(ShellAction::Script(ScriptAction::Reset { index }));
        }
        drop(disabled);
        if ui.is_item_hovered() {
            ui.tooltip_text(
                "Reset this script's hotkey, enable state, and all custom settings to their defaults.",
            );
        }
    } else {
        render_builtin_revert_button(ui, state);
    }
    #[cfg(not(target_os = "linux"))]
    render_builtin_revert_button(ui, state);

    ui.same_line_with_pos(right_child_width - PROFILES_BUTTON_WIDTH);
    nudge_footer_y(ui);
    let profile_action =
        profile_browser::render(ui, &mut state.profile_browser, PROFILES_BUTTON_WIDTH, theme)
            .map(ShellAction::Profile);
    footer_action.or(profile_action)
}

fn render_builtin_revert_button(ui: &Ui, state: &mut ShellState) {
    let Some(section) = usize::try_from(state.selected_section).ok().filter(|index| *index < 16)
    else {
        return;
    };
    if ui.button("Revert Module") {
        state.reset_section_to_defaults(section);
    }
    if ui.is_item_hovered()
        && let Some(title) = sidebar::section_title(section)
    {
        ui.tooltip_text(format!("Reset all settings for '{title}' to their defaults."));
    }
}

fn nudge_footer_y(ui: &Ui) {
    ui.set_cursor_pos_y(ui.cursor_pos_y() + FOOTER_CONTROL_Y_OFFSET);
}

#[cfg(test)]
mod profile_adapter_tests {
    #![allow(clippy::expect_used)]

    use super::ShellState;
    use crate::warning_toast;
    use serde_json::{Map, json};
    use smu_profile::payload::PreservedProfilePayload;

    const CPP_PAYLOAD_FIXTURE: &str =
        include_str!("../../../../compat/profiles/payload_cases.json");
    const CPP_RESET_SECTION_FIXTURE: &str =
        include_str!("../../../../compat/profiles/reset_section_cases.json");

    fn cpp_payload_fixture() -> serde_json::Value {
        serde_json::from_str(CPP_PAYLOAD_FIXTURE).expect("C++ payload fixture should parse")
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_input_setup_gate_disables_and_restores_live_macro_runtime() {
        let mut state = ShellState::default();
        assert!(state.freeze_controller_settings().macro_enabled);
        state.set_linux_input_setup_status(crate::linux_input_setup::LinuxInputSetupStatus {
            required: true,
            ..crate::linux_input_setup::LinuxInputSetupStatus::default()
        });
        assert!(!state.freeze_controller_settings().macro_enabled);
        assert!(!state.script_runtime_settings().macro_enabled);
        state.set_linux_input_setup_status(
            crate::linux_input_setup::LinuxInputSetupStatus::default(),
        );
        assert!(state.freeze_controller_settings().macro_enabled);
        assert!(state.script_runtime_settings().macro_enabled);
    }

    #[test]
    fn cpp_generated_apply_cases_match_live_shell_adapter() {
        let fixture = cpp_payload_fixture();
        for case in fixture["apply_cases"].as_array().expect("apply cases should be an array") {
            if !case["success"].as_bool().unwrap_or(false) {
                continue;
            }
            let name = case["name"].as_str().unwrap_or("unnamed");
            let payload = PreservedProfilePayload::from_value(&case["payload"])
                .expect("successful C++ apply payload should validate in Rust");
            let baseline = PreservedProfilePayload::cpp_default();
            let mut state = ShellState::default();
            state.apply_profile_payload(&baseline);
            state.apply_profile_payload(&payload);
            let actual = state
                .overlay_profile_payload(&baseline)
                .expect("live adapter overlay should validate")
                .raw_snapshot();
            assert_eq!(actual, case["result"], "C++ apply case {name}");
        }
    }

    #[test]
    fn cpp_generated_stateful_sequences_match_live_shell_adapter() {
        let fixture = cpp_payload_fixture();
        for case in fixture["sequence_cases"].as_array().expect("sequence cases should be an array")
        {
            let name = case["name"].as_str().unwrap_or("unnamed");
            let payloads = case["payloads"].as_array().expect("sequence payloads array");
            let steps = case["steps"].as_array().expect("sequence steps array");
            assert_eq!(payloads.len(), steps.len(), "fixture shape {name}");
            let baseline = PreservedProfilePayload::cpp_default();
            let mut state = ShellState::default();
            state.apply_profile_payload(&baseline);
            for (index, (payload, step)) in payloads.iter().zip(steps).enumerate() {
                if !step["success"].as_bool().unwrap_or(false) {
                    continue;
                }
                let payload = PreservedProfilePayload::from_value(payload)
                    .expect("successful C++ sequence payload should validate in Rust");
                state.apply_profile_payload(&payload);
                let actual = state
                    .overlay_profile_payload(&baseline)
                    .expect("live sequence overlay should validate")
                    .raw_snapshot();
                assert_eq!(actual, step["payload"], "C++ sequence {name} step {index}");
            }
        }
    }

    #[test]
    fn cpp_generated_section_resets_match_live_shell_adapter() {
        let fixture: serde_json::Value = serde_json::from_str(CPP_RESET_SECTION_FIXTURE)
            .expect("C++ reset-section fixture should parse");
        for case in fixture["cases"].as_array().expect("reset cases should be an array") {
            if !case["success"].as_bool().unwrap_or(false) {
                continue;
            }
            let section = usize::try_from(case["section"].as_u64().expect("section index"))
                .expect("section index should fit usize");
            let input = PreservedProfilePayload::from_value(&case["input"])
                .expect("successful C++ reset input should validate in Rust");
            let baseline = PreservedProfilePayload::cpp_default();
            let mut state = ShellState::default();
            state.apply_profile_payload(&baseline);
            state.apply_profile_payload(&input);
            state.reset_section_to_defaults(section);
            let actual = state
                .overlay_profile_payload(&baseline)
                .expect("reset overlay should validate")
                .raw_snapshot();
            assert_eq!(actual, case["expected"], "C++ reset section {section}");
        }
    }

    #[test]
    fn settings_parity_case_opens_settings_without_changing_default_section() {
        let state = ShellState::for_parity_case(Some("settings-user-reference"));
        assert!(state.settings_open);
        assert_eq!(state.selected_section, -1);
        assert_eq!(state.global.sensitivity(), "0.5");
    }

    #[test]
    fn theme_editor_parity_case_opens_editor_without_changing_default_section() {
        let state = ShellState::for_parity_case(Some("theme-editor-user-reference"));
        assert!(state.theme.editor_open());
        assert_eq!(state.selected_section, -1);
        assert_eq!(state.theme.active().name, "Cyberpunk");
    }

    #[test]
    fn sidebar_no_scroll_parity_case_enables_short_descriptions() {
        let state = ShellState::for_parity_case(Some("sidebar-no-scroll-user-reference"));
        assert!(state.global.short_descriptions());
        assert_eq!(state.selected_section, -1);
    }

    #[test]
    fn builtin_footer_parity_case_selects_freeze_module() {
        let state = ShellState::for_parity_case(Some("builtin-footer-user-reference"));
        assert_eq!(state.selected_section, 0);
        assert!(state.freeze.enabled);
    }

    #[test]
    fn warning_toast_parity_case_queues_only_reference_warning() {
        let state = ShellState::for_parity_case(Some("warning-toast-user-reference"));
        assert_eq!(state.warning_toast.front(), Some(warning_toast::PARITY_WARNING_MESSAGE));
        assert_eq!(state.warning_toast.len(), 1);
    }

    #[test]
    fn cpp_default_profile_apply_overlay_is_raw_stable() {
        let baseline = PreservedProfilePayload::cpp_default();
        let mut state = ShellState::default();
        state.apply_profile_payload(&baseline);
        let round_trip =
            state.overlay_profile_payload(&baseline).expect("C++ default overlay should validate");
        assert_eq!(round_trip.raw_snapshot(), baseline.raw_snapshot());
    }

    #[test]
    fn overlay_removes_empty_optional_extra_instance_arrays() {
        let baseline = PreservedProfilePayload::from_value(&json!({
            "extra_wallhop_instances": [],
            "extra_presskey_instances": [],
            "extra_spamkey_instances": []
        }))
        .expect("optional empty arrays should validate");
        let state = ShellState::default();
        let round_trip =
            state.overlay_profile_payload(&baseline).expect("overlay should remain valid");
        for key in
            ["extra_wallhop_instances", "extra_presskey_instances", "extra_spamkey_instances"]
        {
            assert_eq!(round_trip.raw_value(key), None, "{key}");
        }
    }

    #[test]
    fn profile_metadata_combines_theme_global_and_notification_state() {
        let mut state = ShellState::default();
        state.apply_profile_metadata(&Map::from_iter([
            ("shortdescriptions".to_owned(), json!(true)),
            ("current_theme_index".to_owned(), json!(1)),
            (
                "suppressed_notifications".to_owned(),
                json!(["future_warning", "admin_elevation_warning"]),
            ),
        ]));
        let metadata = state.profile_metadata_overlay();
        assert_eq!(metadata["shortdescriptions"], json!(true));
        assert_eq!(metadata["current_theme_index"], json!(1));
        assert_eq!(
            metadata["suppressed_notifications"],
            json!(["admin_elevation_warning", "future_warning"])
        );
        assert_eq!(metadata["DontShowAdminWarning"], json!(true));
        assert!(metadata["custom_theme"].is_object());
        assert!(metadata["themes"].is_object());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn lag_switch_helper_request_uses_modal_until_admin_warning_is_suppressed() {
        use super::LinuxModal;

        let mut state = ShellState::default();
        state.request_lag_switch_helper_popup();
        assert!(state.linux_modals.pending_open.contains(&LinuxModal::LagSwitchHelper));
        assert!(!state.linux_modals.pending_lag_switch_helper_start);

        state.linux_modals.pending_open.clear();
        assert!(state.set_admin_warning_suppressed(true));
        state.request_lag_switch_helper_popup();
        assert!(!state.linux_modals.pending_open.contains(&LinuxModal::LagSwitchHelper));
        assert!(state.linux_modals.pending_lag_switch_helper_start);
    }

    #[test]
    fn sparse_profile_changes_only_present_keys_and_prefix_section_flags() {
        let mut state = ShellState::default();
        let item_clip_hotkey_before = state.item_clip.hotkey;
        let process_before = state.global.process_selector();
        let payload = PreservedProfilePayload::from_value(&json!({
            "vk_f5": 0x70,
            "section_toggles": [true],
            "unknown_sparse_marker": {"keep": 1}
        }))
        .expect("sparse payload should validate");

        state.apply_profile_payload(&payload);

        assert_eq!(state.item_desync.hotkey.raw(), 0x70);
        assert!(state.freeze.enabled);
        assert!(!state.item_desync.enabled);
        assert_eq!(state.item_clip.hotkey, item_clip_hotkey_before);
        assert_eq!(state.global.process_selector(), process_before);
    }

    #[test]
    fn apply_then_overlay_preserves_unknowns_scripts_and_char_buffer_precedence() {
        let imported_scripts = json!([{"path":"example.lua","enabled":true}]);
        let unknown = json!({"nested":[1,2,3],"text":"preserve me"});
        let baseline = PreservedProfilePayload::from_value(&json!({
            "ItemClipDelay": "77",
            "clip_delay": 5,
            "unknown_profile_marker": unknown,
            "imported_scripts": imported_scripts
        }))
        .expect("baseline should validate");
        let mut state = ShellState::default();
        state.apply_profile_payload(&baseline);

        let round_trip =
            state.overlay_profile_payload(&baseline).expect("live overlay should remain valid");

        assert_eq!(
            round_trip.raw_value("unknown_profile_marker"),
            baseline.raw_value("unknown_profile_marker")
        );
        assert_eq!(
            round_trip.raw_value("imported_scripts"),
            baseline.raw_value("imported_scripts")
        );
        assert_eq!(round_trip.raw_value("ItemClipDelay"), Some(&json!("77")));
        assert_eq!(round_trip.raw_value("clip_delay"), Some(&json!(77)));
        assert_eq!(round_trip.normalized_value("clip_delay"), Some(&json!(77)));
    }
}
