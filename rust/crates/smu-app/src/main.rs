//! Minimal `SDL3` + Dear `ImGui` + `OpenGL3` compatibility spike.

use std::{
    ffi::c_void,
    path::{Path, PathBuf},
    ptr,
};

#[cfg(target_os = "linux")]
use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicBool, Ordering},
};
#[cfg(target_os = "linux")]
use std::thread::JoinHandle;
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

use dear_imgui_rs::Context;
use dear_imgui_sdl3::{Sdl3OpenGl3Backend, enable_native_ime_ui};
use sdl3::{
    event::Event,
    keyboard::Keycode,
    video::{GLProfile, Window, WindowPos},
};
use serde_json::{Map, Value};
use smu_profile::payload::PreservedProfilePayload;

use script_app_service::ScriptAppServices;
#[cfg(target_os = "linux")]
use smu_display::linux::{LinuxDisplayCapture, WaylandPortalCapture};
#[cfg(target_os = "linux")]
use smu_input::linux::EvdevUinputInputDevice;
#[cfg(target_os = "linux")]
use smu_network::{LagSwitchControl, linux::LinuxLagSwitchBackend};
#[cfg(target_os = "linux")]
use smu_process::ProcessDiscovery as _;
#[cfg(target_os = "linux")]
use smu_process::linux::{
    ProcCgroupProcessControl, ProcDiscovery, ProcForegroundProcess, UnixFreezeHelper,
};
#[cfg(target_os = "linux")]
use smu_roblox::{RobloxLogSource, SharedRobloxLogReader};
use smu_scripting::{ScriptManager, ScriptManagerConfig, ScriptServices};

mod bunnyhop;
#[cfg(target_os = "linux")]
mod bunnyhop_runtime;
mod capture;
mod critical_modal;
mod floor_bounce;
#[cfg(target_os = "linux")]
mod floor_bounce_runtime;
mod foreground_ui;
mod freeze;
#[cfg(target_os = "linux")]
mod freeze_runtime;
mod global_settings;
mod hhj;
#[cfg(target_os = "linux")]
mod hhj_runtime;
mod item_clip;
#[cfg(target_os = "linux")]
mod item_clip_runtime;
mod item_desync;
#[cfg(target_os = "linux")]
mod item_desync_runtime;
mod item_unequip;
#[cfg(target_os = "linux")]
mod item_unequip_runtime;
mod keybind;
mod lag_switch;
#[cfg(target_os = "linux")]
mod lag_switch_runtime;
mod laugh_clip;
#[cfg(target_os = "linux")]
mod laugh_clip_runtime;
mod ledge_bounce;
#[cfg(target_os = "linux")]
mod ledge_bounce_runtime;
#[cfg(target_os = "linux")]
mod linux_freeze_authorization;
#[cfg(target_os = "linux")]
mod linux_input_setup;
#[cfg(target_os = "linux")]
mod linux_input_setup_service;
#[cfg(target_os = "linux")]
mod linux_privileged_helper;
mod macro_text;
mod multi_instance;
mod notification_suppression;
mod parity;
mod press_key;
#[cfg(target_os = "linux")]
mod press_key_runtime;
mod profile_adapter;
mod profile_browser;
mod profile_service;
#[cfg(target_os = "linux")]
mod runtime_input;
mod script_app_service;
mod script_profile_bridge;
#[cfg(target_os = "linux")]
mod script_runtime;
mod script_ui;
mod settings_menu;
mod shell;
mod sidebar;
mod spam_key;
#[cfg(target_os = "linux")]
mod spam_key_runtime;
mod speedglitch;
#[cfg(target_os = "linux")]
mod speedglitch_runtime;
#[cfg(target_os = "linux")]
mod target_runtime;
mod theme;
mod tutorial_media;
#[cfg(target_os = "linux")]
mod updater_service;
mod wall_walk;
#[cfg(target_os = "linux")]
mod wall_walk_runtime;
mod walless_lhj;
#[cfg(target_os = "linux")]
mod walless_lhj_runtime;
mod wallhop;
#[cfg(target_os = "linux")]
mod wallhop_runtime;
mod warning_toast;

const WINDOW_TITLE: &str = "Spencer Macro Utilities";
#[cfg(target_os = "macos")]
const MINIMUM_WIDTH: u32 = 1220;
#[cfg(not(target_os = "macos"))]
const MINIMUM_WIDTH: u32 = 1180;
const MINIMUM_HEIGHT: u32 = 700;
const APP_VERSION_TEXT: &str = include_str!("../../../../version");

fn app_version() -> String {
    APP_VERSION_TEXT.trim().to_owned()
}
#[cfg(target_os = "macos")]
const GLSL_VERSION: &str = "#version 150";
#[cfg(not(target_os = "macos"))]
const GLSL_VERSION: &str = "#version 130";

fn initial_profile_dimension(
    environment_variable: &str,
    minimum: u32,
    payload: Option<&PreservedProfilePayload>,
    key: &str,
) -> u32 {
    if let Some(value) =
        std::env::var(environment_variable).ok().and_then(|value| value.parse::<u32>().ok())
    {
        return value.max(minimum);
    }
    payload
        .and_then(|payload| crate::profile_adapter::i32_value(payload, key))
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(minimum)
        .max(minimum)
}

fn apply_profile_window_position(window: &mut Window, payload: &PreservedProfilePayload) {
    let current = window.position();
    let x = crate::profile_adapter::i32_value(payload, "WindowPosX").unwrap_or(current.0);
    let y = crate::profile_adapter::i32_value(payload, "WindowPosY").unwrap_or(current.1);
    if x != 0 || y != 0 {
        let _ = window.set_position(WindowPos::Positioned(x), WindowPos::Positioned(y));
    }
}

fn set_window_always_on_top(window: &Window, enabled: bool) -> bool {
    // SAFETY: `window.raw()` is the live SDL3 window owned by this main thread; SDL documents
    // `SDL_SetWindowAlwaysOnTop` as a synchronous window-property operation on that handle.
    unsafe { sdl3::sys::video::SDL_SetWindowAlwaysOnTop(window.raw(), enabled) }
}

fn apply_shell_window_style(window: &mut Window, shell_state: &shell::ShellState) {
    let _ = window.set_opacity((shell_state.opacity_percent() / 100.0).clamp(0.0, 1.0));
    let _ = set_window_always_on_top(window, shell_state.always_on_top());
}

fn overlay_imported_scripts(overlay: &mut Map<String, Value>, imported_scripts: Option<Value>) {
    if let Some(imported_scripts) = imported_scripts {
        overlay.insert("imported_scripts".to_owned(), imported_scripts);
    }
}

fn live_profile_payload(
    shell_state: &shell::ShellState,
    baseline: &PreservedProfilePayload,
    window: &Window,
    imported_scripts: Option<Value>,
) -> Option<PreservedProfilePayload> {
    let shell_payload = shell_state.overlay_profile_payload(baseline)?;
    let mut overlay = Map::new();
    let (window_x, window_y) = window.position();
    let (window_width, window_height) = window.size();
    crate::profile_adapter::insert_i32(&mut overlay, "WindowPosX", window_x);
    crate::profile_adapter::insert_i32(&mut overlay, "WindowPosY", window_y);
    crate::profile_adapter::insert_u32(&mut overlay, "screen_width", window_width);
    crate::profile_adapter::insert_u32(&mut overlay, "screen_height", window_height);
    overlay_imported_scripts(&mut overlay, imported_scripts);
    shell_payload.with_overlay(&overlay)
}

fn baseline_for_profile_save(
    service: &profile_service::ProfileBrowserService,
    name: &str,
) -> PreservedProfilePayload {
    service
        .current_payload()
        .cloned()
        .or_else(|| service.load_payload(name))
        .unwrap_or_else(PreservedProfilePayload::cpp_default)
}

fn save_live_profile(
    service: &mut profile_service::ProfileBrowserService,
    shell_state: &shell::ShellState,
    window: &Window,
    name: &str,
    imported_scripts: Option<Value>,
) -> Option<PreservedProfilePayload> {
    if name.is_empty() || name == smu_profile::DEFAULT_PROFILE_NAME {
        return None;
    }
    let baseline = baseline_for_profile_save(service, name);
    let payload = live_profile_payload(shell_state, &baseline, window, imported_scripts)?;
    let theme_metadata = shell_state.profile_metadata_overlay();
    if !service.save_payload_with_metadata(name, &payload, &theme_metadata) {
        return None;
    }
    service.mark_loaded(name.to_owned(), payload.clone());
    Some(payload)
}

fn save_current_profile_before_replacement(
    service: &mut profile_service::ProfileBrowserService,
    shell_state: &shell::ShellState,
    window: &Window,
    imported_scripts: Option<Value>,
) -> bool {
    let Some(current_name) = service.current_name().map(str::to_owned) else {
        return true;
    };
    let Some(current_payload) = service.current_payload().cloned() else {
        return true;
    };
    if current_name != smu_profile::DEFAULT_PROFILE_NAME {
        return save_live_profile(service, shell_state, window, &current_name, imported_scripts)
            .is_some();
    }

    let Some(live_payload) =
        live_profile_payload(shell_state, &current_payload, window, imported_scripts)
    else {
        return false;
    };
    let promoted_name = service.next_profile_name();
    if !service.save_payload(&promoted_name, &live_payload) {
        return false;
    }
    service.mark_loaded(promoted_name, live_payload);
    true
}

fn save_profiles_now(
    service: &mut profile_service::ProfileBrowserService,
    shell_state: &shell::ShellState,
    window: &Window,
    imported_scripts: Option<Value>,
) -> bool {
    if service.current_name() == Some(smu_profile::DEFAULT_PROFILE_NAME) {
        let Some(current_payload) = service.current_payload().cloned() else {
            return false;
        };
        let Some(live_payload) =
            live_profile_payload(shell_state, &current_payload, window, imported_scripts.clone())
        else {
            return false;
        };
        let promoted_name = service.next_profile_name();
        if !service.save_payload(&promoted_name, &live_payload) {
            return false;
        }
        service.mark_loaded(promoted_name, live_payload);
    }
    let Some(current_name) = service.current_name().map(str::to_owned) else {
        return false;
    };
    if current_name == smu_profile::DEFAULT_PROFILE_NAME {
        return false;
    }
    save_live_profile(service, shell_state, window, &current_name, imported_scripts).is_some()
}

#[cfg(target_os = "linux")]
fn save_before_privileged_shell_action(
    action: Option<&shell::ShellAction>,
    service: &mut profile_service::ProfileBrowserService,
    shell_state: &shell::ShellState,
    window: &Window,
    script_app: &ScriptAppServices,
) {
    if !matches!(
        action,
        Some(
            shell::ShellAction::PersistNotificationSuppression
                | shell::ShellAction::StartLagSwitchHelper
        )
    ) {
        return;
    }
    let _ = save_profiles_now(service, shell_state, window, Some(script_app.serialize_scripts()));
}

#[derive(Clone, Debug)]
enum ProfileActionOutcome {
    None,
    Saved(PreservedProfilePayload),
    Loaded(PreservedProfilePayload),
}

fn handle_profile_action(
    action: &profile_browser::ProfileAction,
    service: &mut profile_service::ProfileBrowserService,
    shell_state: &mut shell::ShellState,
    window: &Window,
    imported_scripts: Option<Value>,
) -> ProfileActionOutcome {
    match action {
        profile_browser::ProfileAction::SaveTo { name } => {
            let Some(payload) =
                save_live_profile(service, shell_state, window, name, imported_scripts)
            else {
                shell_state.push_critical(format!("Failed to save settings profile '{name}'."));
                return ProfileActionOutcome::None;
            };
            service.refresh_selected(shell_state.profile_browser_mut(), Some(name));
            ProfileActionOutcome::Saved(payload)
        }
        profile_browser::ProfileAction::Load { name } => {
            if !save_current_profile_before_replacement(
                service,
                shell_state,
                window,
                imported_scripts,
            ) {
                return ProfileActionOutcome::None;
            }
            let Some(payload) = service.load_payload(name) else {
                return ProfileActionOutcome::None;
            };
            shell_state.apply_profile_payload(&payload);
            if let Some(metadata) = service.metadata_snapshot() {
                shell_state.apply_profile_metadata(&metadata);
            }
            service.mark_loaded(name.clone(), payload.clone());
            service.refresh_selected(shell_state.profile_browser_mut(), Some(name));
            ProfileActionOutcome::Loaded(payload)
        }
        profile_browser::ProfileAction::Duplicate { source }
            if source == smu_profile::DEFAULT_PROFILE_NAME =>
        {
            let new_name = service.next_profile_name();
            let Some(payload) =
                save_live_profile(service, shell_state, window, &new_name, imported_scripts)
            else {
                return ProfileActionOutcome::None;
            };
            service.refresh_selected(shell_state.profile_browser_mut(), Some(&new_name));
            ProfileActionOutcome::Saved(payload)
        }
        profile_browser::ProfileAction::Duplicate { source } => {
            if !save_current_profile_before_replacement(
                service,
                shell_state,
                window,
                imported_scripts,
            ) {
                return ProfileActionOutcome::None;
            }
            let Some(new_name) = service.duplicate_profile(source) else {
                return ProfileActionOutcome::None;
            };
            let Some(payload) = service.load_payload(&new_name) else {
                return ProfileActionOutcome::None;
            };
            shell_state.apply_profile_payload(&payload);
            if let Some(metadata) = service.metadata_snapshot() {
                shell_state.apply_profile_metadata(&metadata);
            }
            service.mark_loaded(new_name.clone(), payload.clone());
            service.refresh_selected(shell_state.profile_browser_mut(), Some(&new_name));
            ProfileActionOutcome::Loaded(payload)
        }
        _ => {
            service.handle_action(action, shell_state.profile_browser_mut());
            ProfileActionOutcome::None
        }
    }
}

#[cfg(test)]
mod profile_orchestration_tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use serde_json::json;

    struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        fn new(label: &str) -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("smu-app-startup-{label}-{}-{counter}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("startup test directory");
            Self { path }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn initialize_at(path: PathBuf) -> (shell::ShellState, profile_service::ProfileBrowserService) {
        initialize_profile_state_with_service(
            shell::ShellState::default(),
            profile_service::ProfileBrowserService::from_path_for_testing(path),
        )
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn process_found_parity_state_is_explicit_and_isolated() {
        assert!(LinuxParityOptions::from_case(Some("process-found-user-reference")).process_found);
        assert!(
            !LinuxParityOptions::from_case(Some("process-not-found-user-reference")).process_found
        );
        assert!(!LinuxParityOptions::from_case(Some("lag-switch-user-reference")).process_found);
        assert!(!LinuxParityOptions::from_case(None).process_found);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn root_launch_guard_matches_product_policy_and_parity_exception() {
        assert!(root_launch_refused(0, false));
        assert!(!root_launch_refused(0, true));
        assert!(!root_launch_refused(1000, false));
    }

    #[test]
    fn missing_startup_creates_profile1_and_read_only_default_from_cpp_snapshot() {
        let temporary = TestDirectory::new("missing");
        let path = temporary.path.join("settings.json");
        let (_state, service) = initialize_at(path.clone());
        assert_eq!(service.current_name(), Some("Profile 1"));
        assert_eq!(
            service.current_payload().map(PreservedProfilePayload::raw_snapshot),
            Some(PreservedProfilePayload::cpp_default().raw_snapshot())
        );
        let document: Value = serde_json::from_slice(
            &std::fs::read(&path).expect("startup settings should be created"),
        )
        .expect("startup settings should be JSON");
        assert_eq!(
            document[smu_profile::METADATA_KEY][smu_profile::LAST_ACTIVE_PROFILE_KEY],
            json!("Profile 1")
        );
        assert_eq!(document["Profile 1"], PreservedProfilePayload::cpp_default().raw_snapshot());
        assert_eq!(
            document[smu_profile::DEFAULT_PROFILE_NAME],
            PreservedProfilePayload::cpp_default().raw_snapshot()
        );
    }

    #[test]
    fn legacy_startup_materializes_profile1_drops_unknown_legacy_root_and_preserves_scripts() {
        let temporary = TestDirectory::new("legacy");
        let path = temporary.path.join("settings.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({
                "ItemClipDelay": "77",
                "clip_delay": 5,
                "future_legacy_junk": {"drop": true},
                "imported_scripts": [{"path": "legacy.lua", "enabled": true}]
            }))
            .expect("legacy fixture should serialize"),
        )
        .expect("legacy fixture should write");

        let (_state, service) = initialize_at(path.clone());
        assert_eq!(service.current_name(), Some("Profile 1"));
        let payload = service.current_payload().expect("legacy profile should be current");
        assert_eq!(payload.raw_value("ItemClipDelay"), Some(&json!("77")));
        assert_eq!(payload.raw_value("clip_delay"), Some(&json!(77)));
        assert_eq!(payload.raw_value("future_legacy_junk"), None);
        assert_eq!(
            payload.raw_value("imported_scripts"),
            Some(&json!([{"path":"legacy.lua","enabled":true}]))
        );
        let document: Value =
            serde_json::from_slice(&std::fs::read(&path).expect("converted settings should read"))
                .expect("converted settings should be JSON");
        assert!(document.get("ItemClipDelay").is_none());
        assert!(document.get("future_legacy_junk").is_none());
        assert!(document["Profile 1"].is_object());
        assert!(document[smu_profile::DEFAULT_PROFILE_NAME].is_object());
    }

    #[test]
    fn structured_startup_loads_preferred_profile_preserves_unknowns_and_refreshes_default() {
        let temporary = TestDirectory::new("structured");
        let path = temporary.path.join("settings.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({
                "_metadata": {"last_active_profile": "Profile 2", "future_meta": 8},
                "root_future": {"keep": true},
                "Profile 2": {"vk_f5": 0x70, "future_profile": [1,2,3]}
            }))
            .expect("structured fixture should serialize"),
        )
        .expect("structured fixture should write");

        let (state, service) = initialize_at(path.clone());
        assert_eq!(service.current_name(), Some("Profile 2"));
        assert_eq!(state.item_desync_controller_settings().hotkey.raw(), 0x70);
        assert_eq!(
            service.current_payload().and_then(|payload| payload.raw_value("future_profile")),
            Some(&json!([1, 2, 3]))
        );
        let document: Value =
            serde_json::from_slice(&std::fs::read(&path).expect("structured settings should read"))
                .expect("structured settings should be JSON");
        assert_eq!(document["root_future"], json!({"keep": true}));
        assert_eq!(document["_metadata"]["future_meta"], json!(8));
        assert_eq!(document["_metadata"]["last_active_profile"], json!("Profile 2"));
        assert!(document[smu_profile::DEFAULT_PROFILE_NAME].is_object());
    }

    #[test]
    fn structured_container_without_resume_profile_creates_profile1_from_defaults() {
        let temporary = TestDirectory::new("empty-structured");
        let path = temporary.path.join("settings.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({"_metadata": {}, "root_future": 9}))
                .expect("empty structured fixture should serialize"),
        )
        .expect("empty structured fixture should write");

        let (_state, service) = initialize_at(path.clone());
        assert_eq!(service.current_name(), Some("Profile 1"));
        let document: Value = serde_json::from_slice(
            &std::fs::read(&path).expect("created structured settings should read"),
        )
        .expect("created structured settings should be JSON");
        assert_eq!(document["root_future"], json!(9));
        assert_eq!(document["_metadata"]["last_active_profile"], json!("Profile 1"));
        assert!(document["Profile 1"].is_object());
        assert!(document[smu_profile::DEFAULT_PROFILE_NAME].is_object());
    }

    #[test]
    fn unreadable_startup_keeps_bytes_and_only_sets_in_memory_profile1_name() {
        let temporary = TestDirectory::new("unreadable");
        let path = temporary.path.join("settings.json");
        std::fs::write(&path, b"{ broken").expect("unreadable fixture should write");
        let before = std::fs::read(&path).expect("unreadable fixture bytes");

        let (_state, service) = initialize_at(path.clone());
        assert_eq!(service.current_name(), Some("Profile 1"));
        assert!(service.current_payload().is_none());
        assert_eq!(std::fs::read(&path).expect("unreadable fixture should remain"), before);
    }

    #[test]
    fn parity_startup_is_pathless_and_uses_default_profile_identity() {
        let (_state, service) = initial_profile_state(Some("lag-switch-linux-minimum"));
        assert_eq!(service.current_name(), Some(smu_profile::DEFAULT_PROFILE_NAME));
        assert!(service.current_payload().is_some());
    }

    #[test]
    fn imported_script_overlay_only_replaces_profile_state_when_provided() {
        let mut overlay = Map::new();
        overlay_imported_scripts(&mut overlay, None);
        assert!(!overlay.contains_key("imported_scripts"));

        let scripts = json!([{"path":"new.lua","enabled":true}]);
        overlay_imported_scripts(&mut overlay, Some(scripts.clone()));
        assert_eq!(overlay.get("imported_scripts"), Some(&scripts));
    }

    #[test]
    fn save_to_other_profile_uses_current_live_payload_as_preservation_baseline() {
        let temporary =
            std::env::temp_dir().join(format!("smu-app-profile-baseline-{}", std::process::id()));
        let _ = std::fs::remove_file(&temporary);
        std::fs::write(
            &temporary,
            serde_json::to_vec(&json!({
                "_metadata": {"last_active_profile": "Profile A"},
                "Profile A": {"future_only": "from-a"},
                "Profile B": {"future_only": "from-b"}
            }))
            .expect("fixture should serialize"),
        )
        .expect("fixture should write");
        let current = smu_profile::profile_payload_from_file(&temporary, "Profile A")
            .expect("current payload");
        let mut service =
            profile_service::ProfileBrowserService::from_path_for_testing(temporary.clone());
        service.mark_loaded("Profile A".to_owned(), current);
        let baseline = baseline_for_profile_save(&service, "Profile B");
        assert_eq!(baseline.raw_value("future_only"), Some(&json!("from-a")));
        let _ = std::fs::remove_file(&temporary);
        let _ = std::fs::remove_file(format!("{}.bak", temporary.display()));
    }
}

fn configure_gl_attributes(video: &sdl3::VideoSubsystem) {
    let attributes = video.gl_attr();
    #[cfg(target_os = "macos")]
    {
        attributes.set_context_profile(GLProfile::Core);
        attributes.set_context_version(3, 2);
    }
    #[cfg(not(target_os = "macos"))]
    {
        attributes.set_context_profile(GLProfile::Compatibility);
        attributes.set_context_version(3, 0);
    }
    attributes.set_double_buffer(true);
    attributes.set_depth_size(24);
    attributes.set_stencil_size(8);
}

#[cfg(target_os = "linux")]
struct LinuxPlatformServices {
    process_control: Arc<ProcCgroupProcessControl>,
    process_discovery: Arc<ProcDiscovery>,
    process_foreground: Arc<ProcForegroundProcess>,
    display_service: Arc<LinuxDisplayCapture>,
    wayland_portal: Arc<WaylandPortalCapture>,
    input_service: Arc<EvdevUinputInputDevice>,
}

#[cfg(target_os = "linux")]
impl LinuxPlatformServices {
    fn new(
        freeze_authorization: Arc<linux_freeze_authorization::FreezeHelperAuthorizationGate>,
    ) -> Self {
        let process_control = Arc::new(
            ProcCgroupProcessControl::default()
                .with_helper(Arc::new(UnixFreezeHelper::default()), freeze_authorization),
        );
        let process_discovery = Arc::new(ProcDiscovery::default());
        let process_foreground = Arc::new(ProcForegroundProcess::default());
        let display_service = Arc::new(LinuxDisplayCapture::new());
        let wayland_portal = display_service.wayland_portal();
        let input_service = Arc::new(
            EvdevUinputInputDevice::new().with_wayland_portal(Arc::clone(&wayland_portal)),
        );
        Self {
            process_control,
            process_discovery,
            process_foreground,
            display_service,
            wayland_portal,
            input_service,
        }
    }
}

#[cfg(target_os = "linux")]
fn create_linux_platform_services()
-> (Arc<linux_freeze_authorization::FreezeHelperAuthorizationGate>, LinuxPlatformServices) {
    let authorization =
        Arc::new(linux_freeze_authorization::FreezeHelperAuthorizationGate::default());
    let platform = LinuxPlatformServices::new(Arc::clone(&authorization));
    (authorization, platform)
}

#[cfg(target_os = "linux")]
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Default)]
struct ProcessStatusCache {
    selector: String,
    found: bool,
    last_checked: Option<Instant>,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WaylandSettingsTask {
    StartScreenCapture,
    StopScreenCapture,
    StartRemoteDesktop,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Default)]
struct LinuxParityOptions {
    capture: bool,
    process_found: bool,
    imported_script: bool,
}

#[cfg(target_os = "linux")]
impl LinuxParityOptions {
    #[must_use]
    fn from_case(case: Option<&str>) -> Self {
        Self {
            capture: case.is_some(),
            process_found: case == Some("process-found-user-reference"),
            imported_script: case == Some("imported-script-footer-user-reference"),
        }
    }
}

#[cfg(target_os = "linux")]
struct LinuxRuntimeServices {
    lag_switch_backend: Arc<LinuxLagSwitchBackend>,
    lag_switch_enabled: Arc<AtomicBool>,
    freeze_authorization: Arc<linux_freeze_authorization::FreezeHelperAuthorizationGate>,
    _process_control: Arc<ProcCgroupProcessControl>,
    process_discovery: Arc<ProcDiscovery>,
    process_status: Mutex<ProcessStatusCache>,
    parity: LinuxParityOptions,
    _display_service: Arc<LinuxDisplayCapture>,
    wayland_portal: Arc<WaylandPortalCapture>,
    wayland_settings_busy: Arc<AtomicBool>,
    wayland_settings_worker: Mutex<Option<JoinHandle<()>>>,
    input_setup: linux_input_setup_service::LinuxInputSetupService,
    input_service: Arc<EvdevUinputInputDevice>,
    script_app: ScriptAppServices,
    update_service: updater_service::LinuxUpdateService,
    script_runtime_controller: Option<script_runtime::ScriptRuntimeController>,
    lag_switch_controller: Option<lag_switch_runtime::LagSwitchController>,
    bunnyhop_controller: Option<bunnyhop_runtime::BunnyhopController>,
    hhj_controller: Option<hhj_runtime::HhjController>,
    floor_bounce_controller: Option<floor_bounce_runtime::FloorBounceController>,
    freeze_controller: Option<freeze_runtime::FreezeController>,
    item_desync_controller: Option<item_desync_runtime::ItemDesyncController>,
    item_clip_controller: Option<item_clip_runtime::ItemClipController>,
    item_unequip_controller: Option<item_unequip_runtime::ItemUnequipController>,
    laugh_clip_controller: Option<laugh_clip_runtime::LaughClipController>,
    press_key_controller: Option<press_key_runtime::PressKeyController>,
    spam_key_controller: Option<spam_key_runtime::SpamKeyController>,
    ledge_bounce_controller: Option<ledge_bounce_runtime::LedgeBounceController>,
    speedglitch_controller: Option<speedglitch_runtime::SpeedglitchController>,
    wall_walk_controller: Option<wall_walk_runtime::WallWalkController>,
    walless_lhj_controller: Option<walless_lhj_runtime::WallessLhjController>,
    wallhop_controller: Option<wallhop_runtime::WallhopController>,
}

#[cfg(target_os = "linux")]
fn start_bunnyhop_controller(
    platform: &LinuxPlatformServices,
    shell_state: &shell::ShellState,
) -> bunnyhop_runtime::BunnyhopController {
    let input: Arc<dyn smu_input::InputDevice> = platform.input_service.clone();
    let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
    let foreground: Arc<dyn smu_process::ProcessForeground> = platform.process_foreground.clone();
    bunnyhop_runtime::BunnyhopController::start(
        input,
        discovery,
        foreground,
        shell_state.bunnyhop_controller_settings(),
    )
}

#[cfg(target_os = "linux")]
fn start_floor_bounce_controller(
    platform: &LinuxPlatformServices,
    shared_hhj_active: Arc<AtomicBool>,
    shell_state: &shell::ShellState,
) -> floor_bounce_runtime::FloorBounceController {
    let input: Arc<dyn smu_input::InputDevice> = platform.input_service.clone();
    let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
    let foreground: Arc<dyn smu_process::ProcessForeground> = platform.process_foreground.clone();
    let control: Arc<dyn smu_process::ProcessControl> = platform.process_control.clone();
    floor_bounce_runtime::FloorBounceController::start(
        input,
        discovery,
        foreground,
        control,
        shared_hhj_active,
        shell_state.floor_bounce_controller_settings(),
    )
}

#[cfg(target_os = "linux")]
fn start_hhj_controller(
    platform: &LinuxPlatformServices,
    shared_active: Arc<AtomicBool>,
    shell_state: &shell::ShellState,
) -> hhj_runtime::HhjController {
    let input: Arc<dyn smu_input::InputDevice> = platform.input_service.clone();
    let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
    let foreground: Arc<dyn smu_process::ProcessForeground> = platform.process_foreground.clone();
    let control: Arc<dyn smu_process::ProcessControl> = platform.process_control.clone();
    hhj_runtime::HhjController::start(
        input,
        discovery,
        foreground,
        control,
        shared_active,
        shell_state.hhj_controller_settings(),
    )
}

#[cfg(target_os = "linux")]
fn start_lag_switch_controller(
    platform: &LinuxPlatformServices,
    lag_switch_backend: &Arc<LinuxLagSwitchBackend>,
    lag_switch_enabled: &Arc<AtomicBool>,
    shell_state: &shell::ShellState,
) -> lag_switch_runtime::LagSwitchController {
    let input: Arc<dyn smu_input::KeyState> = platform.input_service.clone();
    lag_switch_runtime::LagSwitchController::start(
        input,
        Arc::clone(lag_switch_backend),
        Arc::clone(lag_switch_enabled),
        shell_state.lag_switch_controller_settings(),
    )
}

#[cfg(target_os = "linux")]
fn start_press_key_controller(
    platform: &LinuxPlatformServices,
    shell_state: &shell::ShellState,
) -> press_key_runtime::PressKeyController {
    let input: Arc<dyn smu_input::InputDevice> = platform.input_service.clone();
    let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
    let foreground: Arc<dyn smu_process::ProcessForeground> = platform.process_foreground.clone();
    press_key_runtime::PressKeyController::start(
        input,
        discovery,
        foreground,
        shell_state.press_key_controller_settings(),
    )
}

#[cfg(target_os = "linux")]
fn start_wallhop_controller(
    input_service: &Arc<EvdevUinputInputDevice>,
    process_discovery: &Arc<ProcDiscovery>,
    process_foreground: &Arc<ProcForegroundProcess>,
    shell_state: &shell::ShellState,
) -> wallhop_runtime::WallhopController {
    let input: Arc<dyn smu_input::InputDevice> = input_service.clone();
    let discovery: Arc<dyn smu_process::ProcessDiscovery> = process_discovery.clone();
    let foreground: Arc<dyn smu_process::ProcessForeground> = process_foreground.clone();
    wallhop_runtime::WallhopController::start(
        input,
        discovery,
        foreground,
        shell_state.wallhop_controller_settings(),
    )
}
#[cfg(target_os = "linux")]
fn start_freeze_controller(
    platform: &LinuxPlatformServices,
    shell_state: &shell::ShellState,
) -> freeze_runtime::FreezeController {
    let input: Arc<dyn smu_input::KeyState> = platform.input_service.clone();
    let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
    let foreground: Arc<dyn smu_process::ProcessForeground> = platform.process_foreground.clone();
    let control: Arc<dyn smu_process::ProcessControl> = platform.process_control.clone();
    freeze_runtime::FreezeController::start(
        input,
        discovery,
        foreground,
        control,
        shell_state.freeze_controller_settings(),
    )
}

#[cfg(target_os = "linux")]
fn start_item_desync_controller(
    platform: &LinuxPlatformServices,
    shell_state: &shell::ShellState,
) -> item_desync_runtime::ItemDesyncController {
    let input: Arc<dyn smu_input::InputDevice> = platform.input_service.clone();
    let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
    let foreground: Arc<dyn smu_process::ProcessForeground> = platform.process_foreground.clone();
    item_desync_runtime::ItemDesyncController::start(
        input,
        discovery,
        foreground,
        shell_state.item_desync_controller_settings(),
    )
}

#[cfg(target_os = "linux")]
fn start_item_unequip_controller(
    platform: &LinuxPlatformServices,
    shell_state: &shell::ShellState,
) -> item_unequip_runtime::ItemUnequipController {
    let input: Arc<dyn smu_input::InputDevice> = platform.input_service.clone();
    let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
    let foreground: Arc<dyn smu_process::ProcessForeground> = platform.process_foreground.clone();
    item_unequip_runtime::ItemUnequipController::start(
        input,
        discovery,
        foreground,
        shell_state.item_unequip_controller_settings(),
    )
}

#[cfg(target_os = "linux")]
fn start_item_clip_controller(
    platform: &LinuxPlatformServices,
    shell_state: &shell::ShellState,
) -> item_clip_runtime::ItemClipController {
    let input: Arc<dyn smu_input::InputDevice> = platform.input_service.clone();
    let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
    let foreground: Arc<dyn smu_process::ProcessForeground> = platform.process_foreground.clone();
    item_clip_runtime::ItemClipController::start(
        input,
        discovery,
        foreground,
        shell_state.item_clip_controller_settings(),
    )
}

#[cfg(target_os = "linux")]
fn start_chat_macro_controllers(
    platform: &LinuxPlatformServices,
    shell_state: &shell::ShellState,
) -> (item_unequip_runtime::ItemUnequipController, laugh_clip_runtime::LaughClipController) {
    (
        start_item_unequip_controller(platform, shell_state),
        start_laugh_clip_controller(platform, shell_state),
    )
}

#[cfg(target_os = "linux")]
fn start_laugh_clip_controller(
    platform: &LinuxPlatformServices,
    shell_state: &shell::ShellState,
) -> laugh_clip_runtime::LaughClipController {
    let input: Arc<dyn smu_input::InputDevice> = platform.input_service.clone();
    let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
    let foreground: Arc<dyn smu_process::ProcessForeground> = platform.process_foreground.clone();
    laugh_clip_runtime::LaughClipController::start(
        input,
        discovery,
        foreground,
        shell_state.laugh_clip_controller_settings(),
    )
}

#[cfg(target_os = "linux")]
fn start_ledge_bounce_controller(
    platform: &LinuxPlatformServices,
    shell_state: &shell::ShellState,
) -> ledge_bounce_runtime::LedgeBounceController {
    let input: Arc<dyn smu_input::InputDevice> = platform.input_service.clone();
    let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
    let foreground: Arc<dyn smu_process::ProcessForeground> = platform.process_foreground.clone();
    ledge_bounce_runtime::LedgeBounceController::start(
        input,
        discovery,
        foreground,
        shell_state.ledge_bounce_controller_settings(),
    )
}

#[cfg(target_os = "linux")]
fn start_spam_key_controller(
    platform: &LinuxPlatformServices,
    shell_state: &shell::ShellState,
) -> spam_key_runtime::SpamKeyController {
    let input: Arc<dyn smu_input::InputDevice> = platform.input_service.clone();
    let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
    let foreground: Arc<dyn smu_process::ProcessForeground> = platform.process_foreground.clone();
    spam_key_runtime::SpamKeyController::start(
        input,
        discovery,
        foreground,
        shell_state.spam_key_controller_settings(),
    )
}

#[cfg(target_os = "linux")]
fn start_speedglitch_controller(
    input_service: &Arc<EvdevUinputInputDevice>,
    process_discovery: &Arc<ProcDiscovery>,
    process_foreground: &Arc<ProcForegroundProcess>,
    shell_state: &shell::ShellState,
) -> speedglitch_runtime::SpeedglitchController {
    let input: Arc<dyn smu_input::InputDevice> = input_service.clone();
    let discovery: Arc<dyn smu_process::ProcessDiscovery> = process_discovery.clone();
    let foreground: Arc<dyn smu_process::ProcessForeground> = process_foreground.clone();
    speedglitch_runtime::SpeedglitchController::start(
        input,
        discovery,
        foreground,
        shell_state.speedglitch_controller_settings(),
    )
}
#[cfg(target_os = "linux")]
fn start_wall_walk_controller(
    input_service: &Arc<EvdevUinputInputDevice>,
    process_discovery: &Arc<ProcDiscovery>,
    process_foreground: &Arc<ProcForegroundProcess>,
    shell_state: &shell::ShellState,
) -> wall_walk_runtime::WallWalkController {
    let input: Arc<dyn smu_input::InputDevice> = input_service.clone();
    let discovery: Arc<dyn smu_process::ProcessDiscovery> = process_discovery.clone();
    let foreground: Arc<dyn smu_process::ProcessForeground> = process_foreground.clone();
    wall_walk_runtime::WallWalkController::start(
        input,
        discovery,
        foreground,
        shell_state.wall_walk_controller_settings(),
    )
}
#[cfg(target_os = "linux")]
fn start_walless_lhj_controller(
    input_service: &Arc<EvdevUinputInputDevice>,
    process_discovery: &Arc<ProcDiscovery>,
    process_foreground: &Arc<ProcForegroundProcess>,
    process_control: &Arc<ProcCgroupProcessControl>,
    shell_state: &shell::ShellState,
) -> walless_lhj_runtime::WallessLhjController {
    let input: Arc<dyn smu_input::InputDevice> = input_service.clone();
    let discovery: Arc<dyn smu_process::ProcessDiscovery> = process_discovery.clone();
    let foreground: Arc<dyn smu_process::ProcessForeground> = process_foreground.clone();
    let control: Arc<dyn smu_process::ProcessControl> = process_control.clone();
    walless_lhj_runtime::WallessLhjController::start(
        input,
        discovery,
        foreground,
        control,
        shell_state.walless_lhj_controller_settings(),
    )
}
#[cfg(target_os = "linux")]
fn start_script_services(
    platform: &LinuxPlatformServices,
    lag_switch_backend: &Arc<LinuxLagSwitchBackend>,
    roblox_log: Arc<dyn RobloxLogSource>,
    saved_settings: Arc<profile_adapter::LiveSavedSettings>,
    shell_state: &shell::ShellState,
) -> (Arc<ScriptManager>, Option<script_runtime::ScriptRuntimeController>) {
    let services = ScriptServices {
        input: Some(platform.input_service.clone()),
        display: Some(platform.display_service.clone()),
        process_discovery: Some(platform.process_discovery.clone()),
        process_control: Some(platform.process_control.clone()),
        network: Some(lag_switch_backend.clone()),
        roblox_log: Some(roblox_log),
        saved_settings: Some(saved_settings),
        logger: None,
    };
    let (selector, take_all_process_ids) = shell_state.script_process_target();
    let mut config = ScriptManagerConfig::new(app_version());
    config.process_selector.clone_from(&selector);
    config.take_all_process_ids = take_all_process_ids;
    let manager = Arc::new(ScriptManager::new(config, services));
    let input: Arc<dyn smu_input::InputDevice> = platform.input_service.clone();
    let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
    let foreground: Arc<dyn smu_process::ProcessForeground> = platform.process_foreground.clone();
    let controller = Some(script_runtime::ScriptRuntimeController::start(
        input,
        discovery,
        foreground,
        Arc::clone(&manager),
        shell_state.script_runtime_settings(),
    ));
    (manager, controller)
}

#[cfg(not(target_os = "linux"))]
fn start_portable_script_app(
    saved_settings: Arc<profile_adapter::LiveSavedSettings>,
    shell_state: &shell::ShellState,
    parity_case: Option<&str>,
) -> ScriptAppServices {
    let (selector, take_all_process_ids) = shell_state.script_process_target();
    let mut config = ScriptManagerConfig::new(app_version());
    config.process_selector = selector;
    config.take_all_process_ids = take_all_process_ids;
    let services = ScriptServices {
        saved_settings: Some(saved_settings.clone()),
        ..ScriptServices::default()
    };
    ScriptAppServices::new(
        Arc::new(ScriptManager::new(config, services)),
        saved_settings,
        parity_case == Some("imported-script-footer-user-reference"),
    )
}

#[cfg(target_os = "linux")]
impl LinuxRuntimeServices {
    fn new(
        shell_state: &shell::ShellState,
        saved_settings: Arc<profile_adapter::LiveSavedSettings>,
        updates_enabled: bool,
        parity: LinuxParityOptions,
    ) -> Self {
        let roblox_log: Arc<dyn RobloxLogSource> = Arc::new(SharedRobloxLogReader::default());
        let lag_switch_backend =
            Arc::new(LinuxLagSwitchBackend::new(Some(Arc::clone(&roblox_log))));
        let lag_switch_enabled = Arc::new(AtomicBool::new(false));
        let (freeze_authorization, platform) = create_linux_platform_services();
        let input_setup = linux_input_setup_service::LinuxInputSetupService::new(
            Arc::clone(&platform.input_service),
            updates_enabled,
        );
        let update_service =
            updater_service::LinuxUpdateService::new(app_version(), updates_enabled);
        let (script_manager, script_runtime_controller) = start_script_services(
            &platform,
            &lag_switch_backend,
            roblox_log,
            Arc::clone(&saved_settings),
            shell_state,
        );
        let script_app =
            ScriptAppServices::new(script_manager, saved_settings, parity.imported_script);
        let hhj_active = Arc::new(AtomicBool::new(false));
        let bunnyhop_controller = start_bunnyhop_controller(&platform, shell_state);
        let hhj_controller = start_hhj_controller(&platform, Arc::clone(&hhj_active), shell_state);
        let floor_bounce_controller =
            start_floor_bounce_controller(&platform, Arc::clone(&hhj_active), shell_state);
        let lag_switch_controller = start_lag_switch_controller(
            &platform,
            &lag_switch_backend,
            &lag_switch_enabled,
            shell_state,
        );
        let freeze_controller = start_freeze_controller(&platform, shell_state);
        let item_desync_controller = start_item_desync_controller(&platform, shell_state);
        let item_clip_controller = start_item_clip_controller(&platform, shell_state);
        let (item_unequip_controller, laugh_clip_controller) =
            start_chat_macro_controllers(&platform, shell_state);
        let press_key_controller = start_press_key_controller(&platform, shell_state);
        let spam_key_controller = start_spam_key_controller(&platform, shell_state);
        let ledge_bounce_controller = start_ledge_bounce_controller(&platform, shell_state);
        let speedglitch_controller = start_speedglitch_controller(
            &platform.input_service,
            &platform.process_discovery,
            &platform.process_foreground,
            shell_state,
        );
        let wall_walk_controller = start_wall_walk_controller(
            &platform.input_service,
            &platform.process_discovery,
            &platform.process_foreground,
            shell_state,
        );
        let walless_lhj_controller = start_walless_lhj_controller(
            &platform.input_service,
            &platform.process_discovery,
            &platform.process_foreground,
            &platform.process_control,
            shell_state,
        );
        let wallhop_controller = start_wallhop_controller(
            &platform.input_service,
            &platform.process_discovery,
            &platform.process_foreground,
            shell_state,
        );
        Self {
            lag_switch_backend,
            lag_switch_enabled,
            freeze_authorization,
            _process_control: platform.process_control,
            process_discovery: platform.process_discovery,
            process_status: Mutex::new(ProcessStatusCache::default()),
            parity,
            _display_service: platform.display_service,
            wayland_portal: platform.wayland_portal,
            wayland_settings_busy: Arc::new(AtomicBool::new(false)),
            wayland_settings_worker: Mutex::new(None),
            input_setup,
            input_service: platform.input_service,
            script_app,
            update_service,
            script_runtime_controller,
            lag_switch_controller: Some(lag_switch_controller),
            bunnyhop_controller: Some(bunnyhop_controller),
            hhj_controller: Some(hhj_controller),
            floor_bounce_controller: Some(floor_bounce_controller),
            freeze_controller: Some(freeze_controller),
            item_desync_controller: Some(item_desync_controller),
            item_clip_controller: Some(item_clip_controller),
            item_unequip_controller: Some(item_unequip_controller),
            laugh_clip_controller: Some(laugh_clip_controller),
            press_key_controller: Some(press_key_controller),
            spam_key_controller: Some(spam_key_controller),
            ledge_bounce_controller: Some(ledge_bounce_controller),
            speedglitch_controller: Some(speedglitch_controller),
            wall_walk_controller: Some(wall_walk_controller),
            walless_lhj_controller: Some(walless_lhj_controller),
            wallhop_controller: Some(wallhop_controller),
        }
    }

    fn process_found(&self, selector: &str) -> bool {
        let mut cache = lock(&self.process_status);
        let due = cache.selector != selector
            || cache.last_checked.is_none_or(|checked| checked.elapsed() >= Duration::from_secs(1));
        if due {
            selector.clone_into(&mut cache.selector);
            cache.found =
                self.process_discovery.find_main_process(selector).ok().flatten().is_some();
            cache.last_checked = Some(Instant::now());
        }
        cache.found
    }

    fn reap_wayland_settings_worker(&self) {
        let mut worker = lock(&self.wayland_settings_worker);
        if worker.as_ref().is_some_and(JoinHandle::is_finished)
            && let Some(worker) = worker.take()
        {
            let _ = worker.join();
        }
    }

    fn start_wayland_settings_task(&self, task: WaylandSettingsTask) {
        self.reap_wayland_settings_worker();
        if self
            .wayland_settings_busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let portal = Arc::clone(&self.wayland_portal);
        let busy = Arc::clone(&self.wayland_settings_busy);
        let worker = std::thread::spawn(move || {
            match task {
                WaylandSettingsTask::StartScreenCapture => {
                    let _ = portal.start();
                }
                WaylandSettingsTask::StopScreenCapture => portal.stop(),
                WaylandSettingsTask::StartRemoteDesktop => {
                    let _ = portal.start_remote_desktop();
                }
            }
            busy.store(false, Ordering::Release);
        });
        *lock(&self.wayland_settings_worker) = Some(worker);
    }

    fn settings_platform_status(&self) -> settings_menu::PlatformStatus {
        let show_wayland = std::env::var("XDG_SESSION_TYPE")
            .is_ok_and(|value| value.eq_ignore_ascii_case("wayland"))
            || std::env::var_os("WAYLAND_DISPLAY").is_some();
        settings_menu::PlatformStatus {
            show_wayland,
            wayland_supported: self.wayland_portal.is_supported(),
            wayland_active: self.wayland_portal.is_active(),
            remote_desktop_active: self.wayland_portal.has_remote_desktop_control(),
            wayland_busy: self.wayland_settings_busy.load(Ordering::Acquire),
            wayland_status: self.wayland_portal.status(),
        }
    }

    fn prepare_shell(&self, shell_state: &mut shell::ShellState) {
        self.reap_wayland_settings_worker();
        if std::env::var("SMU_UI_PARITY_CASE").ok().as_deref()
            != Some(linux_input_setup::PARITY_CASE)
        {
            shell_state.set_linux_input_setup_status(self.input_setup.snapshot());
        }
        if shell_state.keybind_input_poll_needed() {
            let input = keybind::InputSnapshot::capture(
                self.input_service.as_ref(),
                shell_state.keybind_capture_active(),
            );
            shell_state.set_keybind_input(input);
        }
        let selector = shell_state.process_selector();
        let process_found = if self.parity.capture {
            self.parity.process_found
        } else {
            self.process_found(&selector)
        };
        shell_state.set_process_found(process_found);
        shell_state.set_settings_platform_status(self.settings_platform_status());
        let parity_case = std::env::var("SMU_UI_PARITY_CASE").ok();
        let updater_status = settings_menu::parity_update_status(parity_case.as_deref())
            .unwrap_or_else(|| {
                let mut status = self.update_service.prepare();
                if self.parity.capture {
                    // C++ parity mode suppresses update work in StartUpdateCheck(), but the Settings panel
                    // still looks like an enabled, not-yet-checked updater. Keep the Rust oracle state
                    // visually identical without permitting network work.
                    status.enabled = true;
                }
                status
            });
        shell_state.set_settings_updater_status(updater_status);

        let available = self.lag_switch_backend.is_available();
        shell_state.set_lag_switch_backend(lag_switch::BackendState {
            enabled: self.lag_switch_enabled.load(Ordering::Acquire),
            available,
            actively_lagging: self.lag_switch_backend.is_blocking_active(),
        });
        if let Some(controller) = self.lag_switch_controller.as_ref()
            && controller.take_helper_prompt_request()
        {
            shell_state.set_lag_switch_helper_error(self.lag_switch_backend.unsupported_reason());
            shell_state.request_lag_switch_helper_popup();
        }
        if self.freeze_authorization.is_pending() {
            shell_state.request_freeze_helper_popup();
        }
        if self.wayland_portal.has_pending_activation_request() {
            shell_state.request_wayland_screen_capture_popup();
        }
        if self.wayland_portal.has_pending_remote_desktop_activation_request() {
            shell_state.request_wayland_remote_desktop_popup();
        }
    }

    fn finish_shell(
        &self,
        action: Option<&shell::ShellAction>,
        shell_state: &mut shell::ShellState,
        window: &sdl3::video::Window,
    ) {
        if let Some(controller) = self.bunnyhop_controller.as_ref() {
            controller.update_settings(shell_state.bunnyhop_controller_settings());
        }
        if let Some(controller) = self.hhj_controller.as_ref() {
            controller.update_settings(shell_state.hhj_controller_settings());
        }
        if let Some(controller) = self.floor_bounce_controller.as_ref() {
            controller.update_settings(shell_state.floor_bounce_controller_settings());
        }
        if let Some(controller) = self.script_runtime_controller.as_ref() {
            controller.update_settings(shell_state.script_runtime_settings());
        }
        if let Some(controller) = self.lag_switch_controller.as_ref() {
            controller.update_settings(shell_state.lag_switch_controller_settings());
        }
        if let Some(controller) = self.freeze_controller.as_ref() {
            controller.update_settings(shell_state.freeze_controller_settings());
        }
        if let Some(controller) = self.item_desync_controller.as_ref() {
            controller.update_settings(shell_state.item_desync_controller_settings());
        }
        if let Some(controller) = self.item_clip_controller.as_ref() {
            controller.update_settings(shell_state.item_clip_controller_settings());
        }
        if let Some(controller) = self.item_unequip_controller.as_ref() {
            controller.update_settings(shell_state.item_unequip_controller_settings());
        }
        if let Some(controller) = self.laugh_clip_controller.as_ref() {
            controller.update_settings(shell_state.laugh_clip_controller_settings());
        }
        if let Some(controller) = self.press_key_controller.as_ref() {
            controller.update_settings(shell_state.press_key_controller_settings());
        }
        if let Some(controller) = self.spam_key_controller.as_ref() {
            controller.update_settings(shell_state.spam_key_controller_settings());
        }
        if let Some(controller) = self.ledge_bounce_controller.as_ref() {
            controller.update_settings(shell_state.ledge_bounce_controller_settings());
        }
        if let Some(controller) = self.speedglitch_controller.as_ref() {
            controller.update_settings(shell_state.speedglitch_controller_settings());
        }
        if let Some(controller) = self.wall_walk_controller.as_ref() {
            controller.update_settings(shell_state.wall_walk_controller_settings());
        }
        if let Some(controller) = self.walless_lhj_controller.as_ref() {
            controller.update_settings(shell_state.walless_lhj_controller_settings());
        }
        if let Some(controller) = self.wallhop_controller.as_ref() {
            controller.update_settings(shell_state.wallhop_controller_settings());
        }
        self.handle_action(action, shell_state, window);
    }

    fn handle_input_setup_action(
        &self,
        action: &shell::ShellAction,
        shell_state: &mut shell::ShellState,
        window: &sdl3::video::Window,
    ) {
        match action {
            shell::ShellAction::InstallLinuxInputPermissionsGraphical => {
                self.input_setup.install_graphical();
            }
            shell::ShellAction::OpenLinuxInputPermissionTerminal => {
                self.input_setup.open_terminal_installer();
            }
            shell::ShellAction::CopyLinuxInputManualCommand => {
                self.input_setup.copy_manual_command(window);
            }
            shell::ShellAction::OpenLinuxInputSetupDocs => self.input_setup.open_setup_docs(),
            shell::ShellAction::RetryLinuxInputPermissions => {
                self.input_setup.refresh();
                shell_state.set_linux_input_setup_status(self.input_setup.snapshot());
            }
            _ => {}
        }
    }

    fn handle_lag_switch_toggle(&self, shell_state: &mut shell::ShellState) {
        if self.lag_switch_enabled.load(Ordering::Acquire) {
            self.lag_switch_backend.shutdown();
            self.lag_switch_enabled.store(false, Ordering::Release);
            return;
        }
        let config = shell_state.lag_switch_controller_settings().config;
        self.lag_switch_backend.set_base_config(config);
        if self.lag_switch_backend.init() {
            self.lag_switch_enabled.store(true, Ordering::Release);
        } else {
            shell_state.set_lag_switch_helper_error(String::new());
            shell_state.request_lag_switch_helper_popup();
        }
    }

    fn handle_action(
        &self,
        action: Option<&shell::ShellAction>,
        shell_state: &mut shell::ShellState,
        window: &sdl3::video::Window,
    ) {
        match action {
            Some(
                action @ (shell::ShellAction::InstallLinuxInputPermissionsGraphical
                | shell::ShellAction::OpenLinuxInputPermissionTerminal
                | shell::ShellAction::CopyLinuxInputManualCommand
                | shell::ShellAction::OpenLinuxInputSetupDocs
                | shell::ShellAction::RetryLinuxInputPermissions),
            ) => {
                self.handle_input_setup_action(action, shell_state, window);
            }
            Some(shell::ShellAction::ToggleLagSwitchBackend) => {
                self.handle_lag_switch_toggle(shell_state);
            }
            Some(shell::ShellAction::CheckUpdatesFromSettings) => {
                self.update_service.start_check(true);
            }
            Some(shell::ShellAction::RequestInstallUpdateFromSettings) => {
                self.update_service.request_apply();
            }
            Some(shell::ShellAction::ConfirmInstallUpdate) => {
                self.update_service.confirm_apply();
            }
            Some(shell::ShellAction::SkipUpdate) => {
                self.update_service.dismiss_update("Update skipped.");
            }
            Some(shell::ShellAction::CancelUpdate) => {
                self.update_service.dismiss_update("Update cancelled.");
            }
            Some(shell::ShellAction::OpenLatestReleaseFromSettings) => {
                self.update_service.open_latest_release();
            }
            Some(shell::ShellAction::OpenLatestReleaseAndDismiss) => {
                self.update_service.open_latest_release_and_dismiss();
            }
            Some(shell::ShellAction::OpenSponsorFromSettings) => {
                let _ = sdl3::url::open_url(settings_menu::SPONSOR_URL);
            }
            Some(shell::ShellAction::OpenOfficialSource) => {
                let _ = sdl3::url::open_url(global_settings::OFFICIAL_SOURCE_URL);
            }
            Some(shell::ShellAction::StartWaylandScreenCaptureFromSettings) => {
                self.start_wayland_settings_task(WaylandSettingsTask::StartScreenCapture);
            }
            Some(shell::ShellAction::StopWaylandScreenCaptureFromSettings) => {
                self.start_wayland_settings_task(WaylandSettingsTask::StopScreenCapture);
            }
            Some(shell::ShellAction::StartWaylandRemoteDesktopFromSettings) => {
                self.start_wayland_settings_task(WaylandSettingsTask::StartRemoteDesktop);
            }
            Some(shell::ShellAction::StartLagSwitchHelper) => {
                let config = shell_state.lag_switch_controller_settings().config;
                self.lag_switch_backend.set_base_config(config);
                match linux_privileged_helper::start_linux_network_helper_with_graphical_pkexec() {
                    Ok(()) if self.lag_switch_backend.init() => {
                        self.lag_switch_enabled.store(true, Ordering::Release);
                        shell_state.set_lag_switch_helper_error(String::new());
                    }
                    Ok(()) => {
                        shell_state.set_lag_switch_helper_error(
                            self.lag_switch_backend.unsupported_reason(),
                        );
                    }
                    Err(error) => {
                        shell_state.set_lag_switch_helper_error(error);
                    }
                }
            }
            Some(shell::ShellAction::StartFreezeHelper) => {
                let approved =
                    linux_privileged_helper::start_linux_process_helper_with_graphical_pkexec()
                        .is_ok();
                self.freeze_authorization.resolve(approved);
            }
            Some(shell::ShellAction::CancelFreezeHelper) => {
                self.freeze_authorization.resolve(false);
            }
            Some(shell::ShellAction::ApproveWaylandScreenCapture) => {
                self.wayland_portal.approve_activation_request();
            }
            Some(shell::ShellAction::DeclineWaylandScreenCapture) => {
                self.wayland_portal.decline_activation_request();
            }
            Some(shell::ShellAction::ApproveWaylandRemoteDesktop) => {
                self.wayland_portal.approve_remote_desktop_activation_request();
            }
            Some(shell::ShellAction::DeclineWaylandRemoteDesktop) => {
                self.wayland_portal.decline_remote_desktop_activation_request();
            }
            Some(
                shell::ShellAction::Script(_)
                | shell::ShellAction::PersistNotificationSuppression
                | shell::ShellAction::Profile(_),
            )
            | None => {}
        }
    }

    fn update_restart_requested(&self) -> bool {
        self.update_service.restart_requested()
    }

    const fn script_app(&self) -> &ScriptAppServices {
        &self.script_app
    }

    fn shutdown(&self) {
        self.update_service.shutdown();
        if let Some(controller) = self.script_runtime_controller.as_ref() {
            controller.stop();
        }
        if let Some(controller) = self.floor_bounce_controller.as_ref() {
            controller.stop();
        }
        if let Some(controller) = self.hhj_controller.as_ref() {
            controller.stop();
        }
        if let Some(controller) = self.bunnyhop_controller.as_ref() {
            controller.stop();
        }
        if let Some(controller) = self.ledge_bounce_controller.as_ref() {
            controller.stop();
        }
        if let Some(controller) = self.spam_key_controller.as_ref() {
            controller.stop();
        }
        if let Some(controller) = self.press_key_controller.as_ref() {
            controller.stop();
        }
        if let Some(controller) = self.wallhop_controller.as_ref() {
            controller.stop();
        }
        if let Some(controller) = self.walless_lhj_controller.as_ref() {
            controller.stop();
        }
        if let Some(controller) = self.wall_walk_controller.as_ref() {
            controller.stop();
        }
        if let Some(controller) = self.laugh_clip_controller.as_ref() {
            controller.stop();
        }
        if let Some(controller) = self.item_unequip_controller.as_ref() {
            controller.stop();
        }
        if let Some(controller) = self.item_clip_controller.as_ref() {
            controller.stop();
        }
        if let Some(controller) = self.speedglitch_controller.as_ref() {
            controller.stop();
        }
        if let Some(controller) = self.item_desync_controller.as_ref() {
            controller.stop();
        }
        if let Some(controller) = self.freeze_controller.as_ref() {
            controller.stop();
        }
        if let Some(controller) = self.lag_switch_controller.as_ref() {
            controller.stop();
        }
        self.lag_switch_backend.shutdown();
        self.lag_switch_enabled.store(false, Ordering::Release);
        self.wayland_portal.stop();
        let worker = lock(&self.wayland_settings_worker).take();
        if let Some(worker) = worker {
            let _ = worker.join();
        }
        self.wayland_settings_busy.store(false, Ordering::Release);
        self.input_setup.shutdown();
        self.input_service.shutdown();
    }
}

fn initial_profile_state(
    parity_case: Option<&str>,
) -> (shell::ShellState, profile_service::ProfileBrowserService) {
    let shell_state = shell::ShellState::for_parity_case(parity_case);
    let mut profile_service = profile_service::ProfileBrowserService::new(parity_case.is_some());
    if parity_case.is_some() {
        profile_service.mark_loaded(
            smu_profile::DEFAULT_PROFILE_NAME.to_owned(),
            PreservedProfilePayload::cpp_default(),
        );
        return (shell_state, profile_service);
    }
    initialize_profile_state_with_service(shell_state, profile_service)
}

fn initialize_profile_state_with_service(
    mut shell_state: shell::ShellState,
    mut profile_service: profile_service::ProfileBrowserService,
) -> (shell::ShellState, profile_service::ProfileBrowserService) {
    let default_payload = PreservedProfilePayload::cpp_default();
    match profile_service.startup_source() {
        smu_profile::StartupProfileSource::LegacyFlat { payload, .. } => {
            shell_state.apply_profile_payload(&payload);
            let materialized = payload.materialized_serializer_payload();
            profile_service.mark_loaded("Profile 1".to_owned(), materialized.clone());
            if profile_service.replace_legacy_with_profile("Profile 1", &materialized) {
                let _ = profile_service.save_default_payload(&default_payload);
            }
        }
        smu_profile::StartupProfileSource::Structured {
            preferred_name: Some(name),
            preferred_payload: Some(payload),
            ..
        } => {
            shell_state.apply_profile_payload(&payload);
            profile_service.mark_loaded(name, payload);
            let _ = profile_service.save_default_payload(&default_payload);
        }
        smu_profile::StartupProfileSource::Missing
        | smu_profile::StartupProfileSource::Structured { preferred_name: None, .. } => {
            profile_service.mark_loaded("Profile 1".to_owned(), default_payload.clone());
            if profile_service.save_payload("Profile 1", &default_payload) {
                let _ = profile_service.save_default_payload(&default_payload);
            }
        }
        smu_profile::StartupProfileSource::Structured {
            preferred_name: Some(_),
            preferred_payload: None,
            ..
        }
        | smu_profile::StartupProfileSource::Unreadable
        | smu_profile::StartupProfileSource::Unsupported { .. } => {
            profile_service.mark_current_name_only("Profile 1".to_owned());
        }
    }
    if let Some(metadata) = profile_service.metadata_snapshot() {
        shell_state.apply_profile_metadata(&metadata);
    }
    (shell_state, profile_service)
}

fn initial_saved_settings(
    profile_service: &profile_service::ProfileBrowserService,
) -> Arc<profile_adapter::LiveSavedSettings> {
    let settings = Arc::new(profile_adapter::LiveSavedSettings::default());
    if let Some(payload) = profile_service.current_payload() {
        settings.replace(payload);
    }
    settings
}

fn maybe_autosave_profile_selection(
    profile_service: &mut profile_service::ProfileBrowserService,
    shell_state: &mut shell::ShellState,
    window: &Window,
    script_app: &ScriptAppServices,
) {
    let should_save = shell_state
        .profile_browser_mut()
        .take_selection_autosave_request(profile_service.current_name());
    if !should_save {
        return;
    }
    let Some(current_name) = profile_service.current_name().map(str::to_owned) else {
        return;
    };
    if let Some(payload) = save_live_profile(
        profile_service,
        shell_state,
        window,
        &current_name,
        Some(script_app.serialize_scripts()),
    ) {
        script_app.update_saved_settings(&payload);
    }
}

fn save_profiles_before_shutdown(
    parity_case: Option<&str>,
    profile_service: &mut profile_service::ProfileBrowserService,
    shell_state: &shell::ShellState,
    window: &Window,
    script_app: &ScriptAppServices,
) {
    if parity_case.is_none() {
        let _ = save_profiles_now(
            profile_service,
            shell_state,
            window,
            Some(script_app.serialize_scripts()),
        );
    }
}

fn persist_imported_script(
    script_app: &ScriptAppServices,
    profile_service: &mut profile_service::ProfileBrowserService,
    shell_state: &mut shell::ShellState,
    window: &Window,
) {
    if save_profiles_now(profile_service, shell_state, window, Some(script_app.serialize_scripts()))
    {
        if let Some(payload) = profile_service.current_payload() {
            script_app.update_saved_settings(payload);
        }
        profile_service.refresh_selected(shell_state.profile_browser_mut(), None);
    } else {
        shell_state
            .script_ui_mut()
            .set_import_error("Script imported, but the settings file could not be updated.");
    }
}

#[cfg(target_os = "linux")]
const ROOT_LAUNCH_MESSAGE: &str = "Spencer Macro Utilities must not be run as root.

Running a desktop GUI as root can break access to your Wayland/X11 session and exposes the entire application to unnecessary administrator privileges. Run SMU normally as your desktop user; when Linux requires elevated access, SMU will request it only for the specific temporary helper that needs it.";

#[cfg(target_os = "linux")]
#[must_use]
const fn root_launch_refused(effective_uid: u32, parity_capture: bool) -> bool {
    effective_uid == 0 && !parity_capture
}

#[cfg(target_os = "linux")]
fn show_root_launch_error() {
    eprintln!("Spencer Macro Utilities: {ROOT_LAUNCH_MESSAGE}");
    if let Ok(sdl) = sdl3::init()
        && sdl.video().is_ok()
    {
        let _ = sdl3::messagebox::show_simple_message_box(
            sdl3::messagebox::MessageBoxFlag::ERROR,
            "Do Not Run Spencer Macro Utilities as Root",
            ROOT_LAUNCH_MESSAGE,
            Option::<&Window>::None,
        );
    }
}

fn initialize_sdl()
-> Result<(sdl3::Sdl, sdl3::VideoSubsystem, sdl3::EventPump), Box<dyn std::error::Error>> {
    let sdl = sdl3::init()?;
    let video = sdl.video()?;
    let events = sdl.event_pump()?;
    enable_native_ime_ui();
    configure_gl_attributes(&video);
    Ok((sdl, video, events))
}

fn create_initial_window(
    video: &sdl3::VideoSubsystem,
    parity_case: Option<&str>,
    profile_service: &profile_service::ProfileBrowserService,
    shell_state: &shell::ShellState,
) -> Result<Window, Box<dyn std::error::Error>> {
    let startup_payload = profile_service.current_payload();
    let width = initial_profile_dimension(
        "SMU_UI_CAPTURE_WIDTH",
        MINIMUM_WIDTH,
        startup_payload,
        "screen_width",
    );
    let height = initial_profile_dimension(
        "SMU_UI_CAPTURE_HEIGHT",
        MINIMUM_HEIGHT,
        startup_payload,
        "screen_height",
    );
    let mut window = video.window(WINDOW_TITLE, width, height).opengl().resizable().build()?;
    window.set_minimum_size(MINIMUM_WIDTH, MINIMUM_HEIGHT)?;
    if parity_case.is_none()
        && let Some(payload) = startup_payload
    {
        apply_profile_window_position(&mut window, payload);
    }
    apply_shell_window_style(&mut window, shell_state);
    Ok(window)
}

struct UiLoopContext<'a> {
    events: &'a mut sdl3::EventPump,
    backend: &'a mut Sdl3OpenGl3Backend,
    imgui: &'a mut Context,
    window: &'a mut Window,
    gl_context: &'a sdl3::video::GLContext,
    capture_path: Option<&'a std::path::Path>,
    shell_state: &'a mut shell::ShellState,
    profile_service: &'a mut profile_service::ProfileBrowserService,
    script_app: &'a ScriptAppServices,
    #[cfg(target_os = "linux")]
    linux_services: &'a LinuxRuntimeServices,
}

fn stabilize_parity_delta_time(imgui: &mut Context, capture_path: Option<&Path>) {
    if capture_path.is_some() {
        // The C++ parity oracle paces active frames at 60 FPS. Rust capture frames otherwise run
        // back-to-back, which leaves time-based Dear ImGui animations at a different third frame.
        imgui.io_mut().set_delta_time(1.0 / 60.0);
    }
}

fn prepare_parity_frame(imgui: &mut Context, capture_path: Option<&Path>) {
    if std::env::var("SMU_UI_PARITY_CASE").ok().as_deref()
        == Some("theme-cyberpunk-hover-user-reference")
    {
        imgui.io_mut().add_mouse_pos_event([100.0, 160.0]);
    }
    stabilize_parity_delta_time(imgui, capture_path);
}

fn run_ui_loop(context: UiLoopContext<'_>) -> Result<(), Box<dyn std::error::Error>> {
    let UiLoopContext {
        events,
        backend,
        imgui,
        window,
        gl_context,
        capture_path,
        shell_state,
        profile_service,
        script_app,
        #[cfg(target_os = "linux")]
        linux_services,
    } = context;

    let mut rendered_frames = 0_u32;
    'main: loop {
        for event in events.poll_iter() {
            backend.process_event(imgui, &event)?;
            let should_quit = matches!(event, Event::Quit { .. })
                || (matches!(event, Event::KeyDown { keycode: Some(Keycode::Escape), .. })
                    && !shell_state.input_capture_active());
            if should_quit {
                break 'main;
            }
        }

        shell_state.apply_theme_to_context(imgui);
        backend.new_frame(imgui)?;
        prepare_parity_frame(imgui, capture_path);
        let frame = imgui.begin_frame();
        let ui = frame.ui();
        profile_service.prepare_shell(shell_state.profile_browser_mut());
        script_app.prepare_ui(shell_state.script_ui_mut());
        shell_state
            .set_external_input_capture(script_app.input_capture_active(shell_state.script_ui()));
        #[cfg(target_os = "linux")]
        linux_services.prepare_shell(shell_state);
        maybe_autosave_profile_selection(profile_service, shell_state, window, script_app);
        let shell_action = shell::render(ui, shell_state);
        if let Some(shell::ShellAction::Profile(profile_action)) = shell_action.as_ref() {
            let outcome = handle_profile_action(
                profile_action,
                profile_service,
                shell_state,
                window,
                Some(script_app.serialize_scripts()),
            );
            match outcome {
                ProfileActionOutcome::Saved(payload) => script_app.update_saved_settings(&payload),
                ProfileActionOutcome::Loaded(payload) => script_app.apply_profile(&payload),
                ProfileActionOutcome::None => {}
            }
        }
        #[cfg(target_os = "linux")]
        save_before_privileged_shell_action(
            shell_action.as_ref(),
            profile_service,
            shell_state,
            window,
            script_app,
        );
        apply_shell_window_style(window, shell_state);
        let (script_selector, script_take_all) = shell_state.script_process_target();
        script_app.set_process_target(script_selector, script_take_all);
        #[cfg(target_os = "linux")]
        linux_services.finish_shell(shell_action.as_ref(), shell_state, window);
        if let Some(shell::ShellAction::Script(script_action)) = shell_action.as_ref()
            && script_app.handle_action(script_action, shell_state.script_ui_mut(), window)
        {
            persist_imported_script(script_app, profile_service, shell_state, window);
        }
        #[cfg(target_os = "linux")]
        if linux_services.update_restart_requested() {
            break 'main;
        }

        let prepared = backend.prepare(frame, || window.gl_make_current(gl_context))?;
        // SAFETY: the SDL OpenGL context is current on this thread and the loaded function pointers
        // come from that context's SDL video subsystem.
        unsafe {
            gl::ClearColor(0.08, 0.09, 0.10, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }
        backend.render_main(prepared)?;
        rendered_frames = rendered_frames.saturating_add(1);

        if let Some(path) = capture_path
            && rendered_frames >= 3
        {
            capture::save_back_buffer(window, path)?;
            break 'main;
        }
        window.gl_swap_window();
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let parity_case = std::env::var("SMU_UI_PARITY_CASE").ok();
    #[cfg(target_os = "linux")]
    {
        // SAFETY: `geteuid` has no preconditions and does not access caller-owned memory.
        let effective_uid = unsafe { libc::geteuid() };
        if root_launch_refused(effective_uid, parity_case.is_some()) {
            show_root_launch_error();
            return Err(std::io::Error::other(ROOT_LAUNCH_MESSAGE).into());
        }
    }
    let capture_path = std::env::var_os("SMU_UI_CAPTURE_PATH").map(PathBuf::from);
    let (mut shell_state, mut profile_service) = initial_profile_state(parity_case.as_deref());
    let saved_settings = initial_saved_settings(&profile_service);

    let (_sdl, video, mut events) = initialize_sdl()?;

    let mut window =
        create_initial_window(&video, parity_case.as_deref(), &profile_service, &shell_state)?;

    let gl_context = window.gl_create_context()?;
    window.gl_make_current(&gl_context)?;
    gl::load_with(|symbol| {
        video
            .gl_get_proc_address(symbol)
            .map_or(ptr::null(), |function| (function as *const ()).cast::<c_void>())
    });

    let mut imgui = Context::create();
    imgui.set_ini_filename::<PathBuf>(None)?;
    theme::load_fonts(&imgui)?;
    shell_state.apply_theme_to_context(&mut imgui);
    let (tutorial_media, tutorial_warnings) =
        tutorial_media::TutorialMedia::load(&mut imgui, parity_case.as_deref());
    shell_state.set_tutorial_media(tutorial_media);
    for warning in tutorial_warnings {
        shell_state.push_warning(warning);
    }

    // SAFETY: `window` and `gl_context` are declared before `imgui`/backend and therefore outlive
    // the native Dear ImGui attachments that retain their pointers.
    let mut backend =
        unsafe { Sdl3OpenGl3Backend::init(&mut imgui, &window, &gl_context, GLSL_VERSION)? };
    #[cfg(target_os = "linux")]
    let linux_services = LinuxRuntimeServices::new(
        &shell_state,
        Arc::clone(&saved_settings),
        parity_case.is_none(),
        LinuxParityOptions::from_case(parity_case.as_deref()),
    );
    #[cfg(target_os = "linux")]
    if parity_case.is_none() && !smu_process::linux::foreground_detection_available() {
        shell_state.push_warning(warning_toast::FOREGROUND_FALLBACK_WARNING);
    }
    #[cfg(target_os = "linux")]
    let script_app = linux_services.script_app();
    #[cfg(not(target_os = "linux"))]
    let portable_script_app = start_portable_script_app(
        Arc::clone(&saved_settings),
        &shell_state,
        parity_case.as_deref(),
    );
    #[cfg(not(target_os = "linux"))]
    let script_app = &portable_script_app;
    if let Some(payload) = profile_service.current_payload() {
        script_app.apply_profile(payload);
    }
    #[cfg(target_os = "linux")]
    run_ui_loop(UiLoopContext {
        events: &mut events,
        backend: &mut backend,
        imgui: &mut imgui,
        window: &mut window,
        gl_context: &gl_context,
        capture_path: capture_path.as_deref(),
        shell_state: &mut shell_state,
        profile_service: &mut profile_service,
        script_app,
        linux_services: &linux_services,
    })?;
    #[cfg(not(target_os = "linux"))]
    run_ui_loop(UiLoopContext {
        events: &mut events,
        backend: &mut backend,
        imgui: &mut imgui,
        window: &mut window,
        gl_context: &gl_context,
        capture_path: capture_path.as_deref(),
        shell_state: &mut shell_state,
        profile_service: &mut profile_service,
        script_app,
    })?;

    save_profiles_before_shutdown(
        parity_case.as_deref(),
        &mut profile_service,
        &shell_state,
        &window,
        script_app,
    );

    #[cfg(target_os = "linux")]
    linux_services.shutdown();
    script_app.shutdown();

    Ok(())
}
