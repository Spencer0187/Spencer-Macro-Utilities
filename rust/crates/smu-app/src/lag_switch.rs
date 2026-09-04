use dear_imgui_rs::Ui;
use serde_json::{Map, Value};
use smu_core::Hotkey;
use smu_profile::payload::PreservedProfilePayload;

#[cfg(target_os = "linux")]
use smu_network::{LagSwitchConfig, LagSwitchTargetMode};

const SUCCESS: [f32; 4] = [0.0, 1.0, 0.50, 1.0];
const WARNING: [f32; 4] = [1.0, 0.50, 0.00, 1.0];
const ERROR: [f32; 4] = [1.0, 0.20, 0.40, 1.0];
#[cfg(target_os = "windows")]
const ACCENT_PRIMARY: [f32; 4] = [0.0, 0.49, 0.62, 1.0];

const LAG_DURATION_INPUT_WIDTH: f32 = 60.0;
const LAG_DURATION_SLIDER_WIDTH: f32 = 300.0;
const UNBLOCK_INPUT_WIDTH: f32 = 50.0;
const FAKE_LAG_DELAY_WIDTH: f32 = 150.0;
#[cfg(target_os = "windows")]
const OVERLAY_ITEM_WIDTH: f32 = 500.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LagSwitchAction {
    ToggleBackend,
}

#[derive(Clone, Copy, Debug)]
pub struct DirectionPair {
    pub inbound: bool,
    pub outbound: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct FilterState {
    pub roblox_only: bool,
    pub use_tcp: bool,
}

#[derive(Debug)]
pub struct AutoUnlagState {
    pub enabled: bool,
    pub max_duration_seconds: f32,
    pub unblock_ms_text: String,
}

#[derive(Clone, Copy, Debug)]
pub struct FakeLagState {
    pub enabled: bool,
    pub delay_ms: i32,
    pub directions: DirectionPair,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug)]
pub struct OverlayState {
    pub shown: bool,
    pub hide_inactive: bool,
    pub use_background: bool,
    pub x: i32,
    pub y: i32,
    pub text_size: i32,
    pub background: [f32; 3],
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Default)]
pub struct OverlayState {
    pub shown: bool,
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
#[derive(Clone, Copy, Debug, Default)]
pub struct OverlayState;

#[derive(Clone, Copy, Debug)]
pub struct BackendState {
    pub enabled: bool,
    pub available: bool,
    pub actively_lagging: bool,
}

#[derive(Debug)]
pub struct LagSwitchUiState {
    pub enabled: bool,
    pub hotkey: Hotkey,
    pub toggle_key_mode: bool,
    #[cfg(target_os = "windows")]
    pub prevent_disconnect: bool,
    pub filter: FilterState,
    pub auto_unlag: AutoUnlagState,
    pub hard_block: DirectionPair,
    pub fake_lag: FakeLagState,
    pub overlay: OverlayState,
    pub backend: BackendState,
}

impl Default for LagSwitchUiState {
    fn default() -> Self {
        Self {
            enabled: false,
            hotkey: crate::lag_switch_runtime::DEFAULT_LAG_SWITCH_HOTKEY,
            toggle_key_mode: true,
            #[cfg(target_os = "windows")]
            prevent_disconnect: true,
            filter: FilterState { roblox_only: true, use_tcp: false },
            auto_unlag: AutoUnlagState {
                enabled: false,
                max_duration_seconds: 9.0,
                unblock_ms_text: "50".to_owned(),
            },
            hard_block: DirectionPair { inbound: true, outbound: true },
            fake_lag: FakeLagState {
                enabled: false,
                delay_ms: 0,
                directions: DirectionPair { inbound: true, outbound: true },
            },
            overlay: default_overlay(),
            backend: BackendState { enabled: false, available: false, actively_lagging: false },
        }
    }
}

#[cfg(target_os = "windows")]
const fn default_overlay() -> OverlayState {
    OverlayState {
        shown: false,
        hide_inactive: false,
        use_background: false,
        x: -1,
        y: 50,
        text_size: 20,
        background: [60.0 / 255.0, 90.0 / 255.0, 160.0 / 255.0],
    }
}

#[cfg(target_os = "linux")]
const fn default_overlay() -> OverlayState {
    OverlayState { shown: false }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
const fn default_overlay() -> OverlayState {
    OverlayState
}

impl LagSwitchUiState {
    pub(crate) fn apply_profile(&mut self, payload: &PreservedProfilePayload) {
        if let Some(v) = crate::profile_adapter::u32_value(payload, "vk_lagswitchkey") {
            self.hotkey = Hotkey::new(v);
        }
        if let Some(v) = crate::profile_adapter::bool_value(payload, "islagswitchswitch") {
            self.toggle_key_mode = v;
        }
        if let Some(v) = crate::profile_adapter::bool_value(payload, "lagswitchoutbound") {
            self.hard_block.outbound = v;
        }
        if let Some(v) = crate::profile_adapter::bool_value(payload, "lagswitchinbound") {
            self.hard_block.inbound = v;
        }
        if let Some(v) = crate::profile_adapter::bool_value(payload, "lagswitchtargetroblox") {
            self.filter.roblox_only = v;
        }
        if let Some(v) = crate::profile_adapter::bool_value(payload, "lagswitchusetcp") {
            self.filter.use_tcp = v;
        }
        if let Some(v) = crate::profile_adapter::bool_value(payload, "lagswitch_autounblock") {
            self.auto_unlag.enabled = v;
        }
        if let Some(v) = crate::profile_adapter::f32_value(payload, "lagswitch_max_duration") {
            self.auto_unlag.max_duration_seconds = v;
        }
        if let Some(v) = crate::profile_adapter::i32_value(payload, "lagswitch_unblock_ms") {
            self.auto_unlag.unblock_ms_text = v.to_string();
        }
        if let Some(v) = crate::profile_adapter::bool_value(payload, "lagswitchlag") {
            self.fake_lag.enabled = v;
        }
        if let Some(v) = crate::profile_adapter::bool_value(payload, "lagswitchlaginbound") {
            self.fake_lag.directions.inbound = v;
        }
        if let Some(v) = crate::profile_adapter::bool_value(payload, "lagswitchlagoutbound") {
            self.fake_lag.directions.outbound = v;
        }
        if let Some(v) = crate::profile_adapter::i32_value(payload, "lagswitchlagdelay") {
            self.fake_lag.delay_ms = v;
        }
        #[cfg(target_os = "windows")]
        {
            if let Some(v) = crate::profile_adapter::bool_value(payload, "prevent_disconnect") {
                self.prevent_disconnect = v;
            }
            if let Some(v) = crate::profile_adapter::bool_value(payload, "show_lag_overlay") {
                self.overlay.shown = v;
            }
            if let Some(v) = crate::profile_adapter::bool_value(payload, "overlay_hide_inactive") {
                self.overlay.hide_inactive = v;
            }
            if let Some(v) = crate::profile_adapter::bool_value(payload, "overlay_use_bg") {
                self.overlay.use_background = v;
            }
            if let Some(v) = crate::profile_adapter::i32_value(payload, "overlay_x") {
                self.overlay.x = v;
            }
            if let Some(v) = crate::profile_adapter::i32_value(payload, "overlay_y") {
                self.overlay.y = v;
            }
            if let Some(v) = crate::profile_adapter::i32_value(payload, "overlay_size") {
                self.overlay.text_size = v;
            }
            if let Some(v) = crate::profile_adapter::f32_value(payload, "overlay_bg_r") {
                self.overlay.background[0] = v;
            }
            if let Some(v) = crate::profile_adapter::f32_value(payload, "overlay_bg_g") {
                self.overlay.background[1] = v;
            }
            if let Some(v) = crate::profile_adapter::f32_value(payload, "overlay_bg_b") {
                self.overlay.background[2] = v;
            }
        }
        #[cfg(target_os = "linux")]
        if let Some(v) = crate::profile_adapter::bool_value(payload, "show_lag_overlay") {
            self.overlay.shown = v;
        }
    }

    pub(crate) fn overlay_profile(&self, overlay: &mut Map<String, Value>) {
        crate::profile_adapter::insert_u32(overlay, "vk_lagswitchkey", self.hotkey.raw());
        crate::profile_adapter::insert_bool(overlay, "islagswitchswitch", self.toggle_key_mode);
        crate::profile_adapter::insert_bool(overlay, "lagswitchoutbound", self.hard_block.outbound);
        crate::profile_adapter::insert_bool(overlay, "lagswitchinbound", self.hard_block.inbound);
        crate::profile_adapter::insert_bool(
            overlay,
            "lagswitchtargetroblox",
            self.filter.roblox_only,
        );
        crate::profile_adapter::insert_bool(overlay, "lagswitchusetcp", self.filter.use_tcp);
        crate::profile_adapter::insert_bool(
            overlay,
            "lagswitch_autounblock",
            self.auto_unlag.enabled,
        );
        crate::profile_adapter::insert_f32(
            overlay,
            "lagswitch_max_duration",
            self.auto_unlag.max_duration_seconds,
        );
        crate::profile_adapter::insert_i32(
            overlay,
            "lagswitch_unblock_ms",
            self.auto_unlag.unblock_ms_text.parse::<i32>().unwrap_or(0),
        );
        crate::profile_adapter::insert_bool(overlay, "lagswitchlag", self.fake_lag.enabled);
        crate::profile_adapter::insert_bool(
            overlay,
            "lagswitchlaginbound",
            self.fake_lag.directions.inbound,
        );
        crate::profile_adapter::insert_bool(
            overlay,
            "lagswitchlagoutbound",
            self.fake_lag.directions.outbound,
        );
        crate::profile_adapter::insert_i32(overlay, "lagswitchlagdelay", self.fake_lag.delay_ms);
        #[cfg(target_os = "windows")]
        {
            crate::profile_adapter::insert_bool(
                overlay,
                "prevent_disconnect",
                self.prevent_disconnect,
            );
            crate::profile_adapter::insert_bool(overlay, "show_lag_overlay", self.overlay.shown);
            crate::profile_adapter::insert_bool(
                overlay,
                "overlay_hide_inactive",
                self.overlay.hide_inactive,
            );
            crate::profile_adapter::insert_bool(
                overlay,
                "overlay_use_bg",
                self.overlay.use_background,
            );
            crate::profile_adapter::insert_i32(overlay, "overlay_x", self.overlay.x);
            crate::profile_adapter::insert_i32(overlay, "overlay_y", self.overlay.y);
            crate::profile_adapter::insert_i32(overlay, "overlay_size", self.overlay.text_size);
            crate::profile_adapter::insert_f32(overlay, "overlay_bg_r", self.overlay.background[0]);
            crate::profile_adapter::insert_f32(overlay, "overlay_bg_g", self.overlay.background[1]);
            crate::profile_adapter::insert_f32(overlay, "overlay_bg_b", self.overlay.background[2]);
        }
        #[cfg(target_os = "linux")]
        crate::profile_adapter::insert_bool(overlay, "show_lag_overlay", self.overlay.shown);
    }

    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn controller_settings(
        &self,
        macro_enabled: bool,
    ) -> crate::lag_switch_runtime::LagSwitchControllerSettings {
        let unblock_duration_ms = self.auto_unlag.unblock_ms_text.parse::<i32>().unwrap_or(0);
        crate::lag_switch_runtime::LagSwitchControllerSettings {
            macro_enabled,
            section_enabled: self.enabled,
            hotkey: self.hotkey,
            toggle_mode: self.toggle_key_mode,
            config: LagSwitchConfig {
                enabled: false,
                currently_blocking: false,
                inbound_hard_block: self.hard_block.inbound,
                outbound_hard_block: self.hard_block.outbound,
                fake_lag_enabled: self.fake_lag.enabled,
                inbound_fake_lag: self.fake_lag.directions.inbound,
                outbound_fake_lag: self.fake_lag.directions.outbound,
                fake_lag_delay_ms: self.fake_lag.delay_ms,
                target_roblox_only: self.filter.roblox_only,
                use_udp: true,
                use_tcp: self.filter.use_tcp,
                prevent_disconnect: false,
                auto_unblock: self.auto_unlag.enabled,
                max_duration_seconds: self.auto_unlag.max_duration_seconds,
                unblock_duration_ms,
                target_mode: if self.filter.roblox_only {
                    LagSwitchTargetMode::Roblox
                } else {
                    LagSwitchTargetMode::All
                },
                remote_ips: Vec::new(),
                remote_ports: Vec::new(),
                include_roblox_dynamic_ips: false,
            },
        }
    }

    /// State visible in the user-provided parity reference crop.
    #[must_use]
    pub fn user_reference() -> Self {
        Self {
            enabled: true,
            toggle_key_mode: true,
            filter: FilterState { roblox_only: false, use_tcp: false },
            auto_unlag: AutoUnlagState {
                enabled: false,
                max_duration_seconds: 9.0,
                unblock_ms_text: "50".to_owned(),
            },
            hard_block: DirectionPair { inbound: false, outbound: false },
            fake_lag: FakeLagState {
                enabled: true,
                delay_ms: 500,
                directions: DirectionPair { inbound: true, outbound: false },
            },
            #[cfg(target_os = "windows")]
            prevent_disconnect: false,
            ..Self::default()
        }
    }
}

pub fn render(ui: &Ui, state: &mut LagSwitchUiState) -> Option<LagSwitchAction> {
    render_shared_header(ui, state);

    #[cfg(target_os = "macos")]
    {
        render_macos_unavailable(ui, state);
        None
    }

    #[cfg(not(target_os = "macos"))]
    {
        render_supported_platform(ui, state)
    }
}

fn render_shared_header(ui: &Ui, state: &mut LagSwitchUiState) {
    ui.text_wrapped("Settings for Lag Switch");
    crate::parity::separator(ui);
    ui.new_line();
    ui.text_wrapped("Keybind:");
    ui.same_line();
    render_keybind_placeholder(ui);

    let color = if state.enabled { SUCCESS } else { ERROR };
    ui.text_colored(color, "Enable This Macro:");
    ui.same_line();
    ui.checkbox("##SectionEnabled", &mut state.enabled);
}

fn render_keybind_placeholder(ui: &Ui) {
    let _ = ui.button("Click to Bind Key");
    ui.same_line();
    ui.set_next_item_width(170.0);
    let mut human = "=".to_owned();
    ui.input_text("##KeyHuman", &mut human).read_only(true).build();
    ui.same_line();
    ui.text("Key Binding");
    ui.same_line();
    ui.set_next_item_width(130.0);
    let mut hex = "0xBB".to_owned();
    ui.input_text("##KeyHex", &mut hex).read_only(true).build();
    ui.same_line();
    ui.text("(Hex)");
}

#[cfg(target_os = "macos")]
fn render_macos_unavailable(ui: &Ui, state: &mut LagSwitchUiState) {
    state.enabled = false;
    state.fake_lag.enabled = false;
    ui.text_wrapped("Network Lag Switch");
    crate::parity::separator(ui);
    ui.text_colored(WARNING, "Unavailable on macOS");
    ui.text_wrapped(
        "Lagswitch on macOS is impossible in this build unless someone pays for Apple's $99/year \
         Developer Program and ships a Developer ID-signed, Apple-entitled Network Extension.",
    );
}

#[cfg(not(target_os = "macos"))]
fn render_supported_platform(ui: &Ui, state: &mut LagSwitchUiState) -> Option<LagSwitchAction> {
    ui.checkbox("Switch from Hold Key to Toggle Key", &mut state.toggle_key_mode);
    crate::parity::separator(ui);
    ui.text_wrapped("Explanation:");
    ui.text_wrapped(
        "This macro allows you to temporarily block or delay Roblox network traffic. Use the \
         direction checkboxes below to choose whether other players stop seeing you, you stop \
         seeing them, or both.",
    );
    crate::parity::separator(ui);

    #[cfg(target_os = "windows")]
    render_prevent_disconnect(ui, state);

    ui.checkbox("Only Lag Switch Roblox", &mut state.filter.roblox_only);
    ui.checkbox("Also Block TCP (Websites)", &mut state.filter.use_tcp);
    crate::parity::separator(ui);

    render_auto_unlag(ui, &mut state.auto_unlag);

    crate::parity::separator(ui);
    ui.checkbox(
        "Block Outbound (Upload/Send) (Players won't be able to see you move)",
        &mut state.hard_block.outbound,
    );
    ui.checkbox(
        "Block Inbound (Download/Recv) (You won't be able to see other players move)",
        &mut state.hard_block.inbound,
    );
    if !state.hard_block.outbound && !state.hard_block.inbound {
        ui.text_colored(
            WARNING,
            "WARNING: Both Inbound and Outbound are unchecked.\nThe Lag Switch will not block any \
             packets.",
        );
    }

    render_fake_lag(ui, &mut state.fake_lag, state.hard_block);
    render_overlay(ui, &mut state.overlay);
    render_backend_status(ui, state.backend).then_some(LagSwitchAction::ToggleBackend)
}

#[cfg(not(target_os = "macos"))]
fn render_auto_unlag(ui: &Ui, state: &mut AutoUnlagState) {
    ui.checkbox("Auto-Unlag (Anti-Kick) (Non-Roblox Games Only)", &mut state.enabled);
    let _disabled = ui.begin_disabled_with_cond(!state.enabled);
    ui.text_wrapped("Automatically stops lagging after this amount of seconds");
    ui.set_next_item_width(LAG_DURATION_INPUT_WIDTH);
    if let Ok(input) =
        ui.input_float_config("##LagFloat").step(0.0).step_fast(0.0).try_display_format("%.2f")
    {
        input.build(&mut state.max_duration_seconds);
    }
    ui.same_line();
    ui.set_next_item_width(LAG_DURATION_SLIDER_WIDTH);
    if let Ok(slider) =
        ui.slider_config("##LagSlider", 0.0_f32, 15.0_f32).try_display_format("%.2f Seconds")
    {
        slider.build(&mut state.max_duration_seconds);
    }
    ui.set_next_item_width(UNBLOCK_INPUT_WIDTH);
    ui.input_text("Modify 50ms Default Unlag Time (MS)", &mut state.unblock_ms_text)
        .chars_decimal(true)
        .chars_no_blank(true)
        .build();
}

#[cfg(target_os = "windows")]
fn render_prevent_disconnect(ui: &Ui, state: &mut LagSwitchUiState) {
    ui.checkbox("Prevent Roblox Disconnection (", &mut state.prevent_disconnect);
    ui.same_line_with_spacing(0.0, 0.0);
    ui.text_colored(ACCENT_PRIMARY, "?");
    ui.same_line_with_spacing(0.0, 0.0);
    ui.text(")");
    // The C++ oracle overlays an invisible tooltip target and then advances the cursor by six
    // logical pixels. Screenshot automation will add a direct geometry assertion for this
    // Windows-only row before the tooltip hit-box is considered complete.
    ui.new_line();
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn render_fake_lag(ui: &Ui, state: &mut FakeLagState, hard_block: DirectionPair) {
    crate::parity::separator(ui);
    ui.checkbox("Fake Lag (Simulate High Ping)", &mut state.enabled);
    let disabled = ui.begin_disabled_with_cond(!state.enabled);
    let indent = ui.begin_indent();
    ui.text("Delay Amount (Milliseconds):");
    ui.same_line();
    ui.set_next_item_width(FAKE_LAG_DELAY_WIDTH);
    ui.input_int_config("##LagDelayInput").step(10).step_fast(100).build(&mut state.delay_ms);
    ui.checkbox("Lag Inbound (Recv)##FakeLag", &mut state.directions.inbound);
    ui.same_line();
    ui.checkbox("Lag Outbound (Send)##FakeLag", &mut state.directions.outbound);
    if state.enabled && !state.directions.inbound && !state.directions.outbound {
        ui.text_colored(WARNING, "Select at least one direction to lag!");
    }
    if state.enabled
        && ((hard_block.outbound && state.directions.outbound)
            || (hard_block.inbound && state.directions.inbound))
    {
        ui.text_colored(
            WARNING,
            "WARNING: Both Blocking and Lagging are enabled for Inbound/Outbound packets.\nBlocking \
             takes priority over lagging.",
        );
    }
    drop(indent);
    drop(disabled);
    crate::parity::separator(ui);
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn render_fake_lag(_ui: &Ui, _state: &mut FakeLagState, _hard_block: DirectionPair) {}

#[cfg(target_os = "windows")]
fn render_overlay(ui: &Ui, state: &mut OverlayState) {
    ui.checkbox("Show Lagswitch Status Overlay", &mut state.shown);
    let _disabled = ui.begin_disabled_with_cond(!state.shown);
    let _indent = ui.begin_indent();
    ui.checkbox("Hide When Not Actively Lagswitching", &mut state.hide_inactive);
    ui.set_next_item_width(OVERLAY_ITEM_WIDTH);
    ui.slider_i32("Overlay X", &mut state.x, -1920, 3840);
    ui.set_next_item_width(OVERLAY_ITEM_WIDTH);
    ui.slider_i32("Overlay Y", &mut state.y, -1080, 2160);
    ui.set_next_item_width(OVERLAY_ITEM_WIDTH);
    ui.slider_i32("Text Size", &mut state.text_size, 10, 100);
    ui.checkbox("Add Background", &mut state.use_background);
    let _background_disabled = ui.begin_disabled_with_cond(!state.use_background);
    ui.set_next_item_width(OVERLAY_ITEM_WIDTH);
    ui.color_edit3("Background Color", &mut state.background);
}

#[cfg(target_os = "linux")]
fn render_overlay(ui: &Ui, state: &mut OverlayState) {
    let _disabled = ui.begin_disabled();
    ui.checkbox("Show Lagswitch Status Overlay (Windows Only)", &mut state.shown);
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn render_overlay(_ui: &Ui, _state: &mut OverlayState) {}

#[cfg(not(target_os = "macos"))]
fn render_backend_status(ui: &Ui, state: BackendState) -> bool {
    ui.new_line();
    crate::parity::separator(ui);

    #[cfg(target_os = "linux")]
    let button_label = if state.enabled {
        "Disable Lagswitch"
    } else if state.available {
        "Enable Lagswitch"
    } else {
        "Enable Lagswitch Helper"
    };
    #[cfg(not(target_os = "linux"))]
    let button_label = if state.enabled { "Disable WinDivert" } else { "Enable WinDivert" };

    let clicked = ui.button(button_label);
    ui.same_line();

    #[cfg(target_os = "linux")]
    if state.available {
        ui.text_colored(SUCCESS, "Helper Ready");
    } else {
        ui.text_colored(ERROR, "Helper Not Running");
    }

    #[cfg(not(target_os = "linux"))]
    if state.enabled {
        ui.text_colored(SUCCESS, "Driver Running");
    } else {
        ui.text_colored(ERROR, "Driver Not Running");
    }

    if state.enabled {
        ui.same_line();
        ui.text(" |  Status: ");
        ui.same_line();
        if state.actively_lagging {
            ui.text_colored(ERROR, "LAGGING");
        } else {
            ui.text_colored(SUCCESS, "Clear");
        }
    }

    clicked
}

#[cfg(test)]
mod tests {
    use super::LagSwitchUiState;

    #[test]
    fn production_defaults_match_cpp_oracle() {
        let state = LagSwitchUiState::default();
        assert!(!state.enabled);
        assert!(state.toggle_key_mode);
        assert!(state.filter.roblox_only);
        assert!(!state.filter.use_tcp);
        assert!(!state.auto_unlag.enabled);
        assert!((state.auto_unlag.max_duration_seconds - 9.0).abs() < f32::EPSILON);
        assert_eq!(state.auto_unlag.unblock_ms_text, "50");
        assert!(state.hard_block.outbound);
        assert!(state.hard_block.inbound);
        assert!(!state.fake_lag.enabled);
        assert_eq!(state.fake_lag.delay_ms, 0);
        assert!(state.fake_lag.directions.inbound);
        assert!(state.fake_lag.directions.outbound);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_controller_config_matches_cpp_ui_mapping() {
        use smu_network::LagSwitchTargetMode;

        let state = LagSwitchUiState::default();
        let settings = state.controller_settings(true);
        assert!(settings.macro_enabled);
        assert!(!settings.section_enabled);
        assert!(settings.toggle_mode);
        assert_eq!(settings.hotkey, crate::lag_switch_runtime::DEFAULT_LAG_SWITCH_HOTKEY);
        assert!(!settings.config.enabled);
        assert!(!settings.config.currently_blocking);
        assert!(settings.config.inbound_hard_block);
        assert!(settings.config.outbound_hard_block);
        assert!(!settings.config.fake_lag_enabled);
        assert!(settings.config.inbound_fake_lag);
        assert!(settings.config.outbound_fake_lag);
        assert_eq!(settings.config.fake_lag_delay_ms, 0);
        assert!(settings.config.target_roblox_only);
        assert_eq!(settings.config.target_mode, LagSwitchTargetMode::Roblox);
        assert!(settings.config.use_udp);
        assert!(!settings.config.use_tcp);
        assert!(!settings.config.prevent_disconnect);
        assert!(!settings.config.auto_unblock);
        assert!((settings.config.max_duration_seconds - 9.0).abs() < f32::EPSILON);
        assert_eq!(settings.config.unblock_duration_ms, 50);
    }

    #[test]
    fn user_reference_state_tracks_supplied_screenshot() {
        let state = LagSwitchUiState::user_reference();
        assert!(state.enabled);
        assert!(!state.filter.roblox_only);
        assert!(!state.hard_block.outbound);
        assert!(!state.hard_block.inbound);
        assert!(state.fake_lag.enabled);
        assert_eq!(state.fake_lag.delay_ms, 500);
        assert!(state.fake_lag.directions.inbound);
        assert!(!state.fake_lag.directions.outbound);
    }
}
