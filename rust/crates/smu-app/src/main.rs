//! Minimal `SDL3` + Dear `ImGui` + `OpenGL3` compatibility spike.

use std::{ffi::c_void, path::PathBuf, ptr};

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
use sdl3::{event::Event, keyboard::Keycode, video::GLProfile};

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

mod bunnyhop;
#[cfg(target_os = "linux")]
mod bunnyhop_runtime;
mod capture;
mod floor_bounce;
#[cfg(target_os = "linux")]
mod floor_bounce_runtime;
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
mod linux_privileged_helper;
mod macro_text;
mod multi_instance;
mod parity;
mod press_key;
#[cfg(target_os = "linux")]
mod press_key_runtime;
#[cfg(target_os = "linux")]
mod runtime_input;
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
mod wall_walk;
#[cfg(target_os = "linux")]
mod wall_walk_runtime;
mod walless_lhj;
#[cfg(target_os = "linux")]
mod walless_lhj_runtime;
mod wallhop;
#[cfg(target_os = "linux")]
mod wallhop_runtime;

const WINDOW_TITLE: &str = "Spencer Macro Utilities";
#[cfg(target_os = "macos")]
const MINIMUM_WIDTH: u32 = 1220;
#[cfg(not(target_os = "macos"))]
const MINIMUM_WIDTH: u32 = 1180;
const MINIMUM_HEIGHT: u32 = 700;
#[cfg(target_os = "macos")]
const GLSL_VERSION: &str = "#version 150";
#[cfg(not(target_os = "macos"))]
const GLSL_VERSION: &str = "#version 130";

fn capture_dimension(variable: &str, minimum: u32) -> u32 {
    std::env::var(variable)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .map_or(minimum, |value| value.max(minimum))
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
    input_ready: bool,
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
        let input_ready = input_service.init().is_ok();
        Self {
            process_control,
            process_discovery,
            process_foreground,
            display_service,
            wayland_portal,
            input_service,
            input_ready,
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
struct LinuxRuntimeServices {
    lag_switch_backend: Arc<LinuxLagSwitchBackend>,
    lag_switch_enabled: Arc<AtomicBool>,
    freeze_authorization: Arc<linux_freeze_authorization::FreezeHelperAuthorizationGate>,
    _process_control: Arc<ProcCgroupProcessControl>,
    process_discovery: Arc<ProcDiscovery>,
    process_status: Mutex<ProcessStatusCache>,
    _display_service: Arc<LinuxDisplayCapture>,
    wayland_portal: Arc<WaylandPortalCapture>,
    wayland_settings_busy: Arc<AtomicBool>,
    wayland_settings_worker: Mutex<Option<JoinHandle<()>>>,
    input_service: Arc<EvdevUinputInputDevice>,
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
) -> Option<bunnyhop_runtime::BunnyhopController> {
    platform.input_ready.then(|| {
        let input: Arc<dyn smu_input::InputDevice> = platform.input_service.clone();
        let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
        let foreground: Arc<dyn smu_process::ProcessForeground> =
            platform.process_foreground.clone();
        bunnyhop_runtime::BunnyhopController::start(
            input,
            discovery,
            foreground,
            shell_state.bunnyhop_controller_settings(),
        )
    })
}

#[cfg(target_os = "linux")]
fn start_floor_bounce_controller(
    platform: &LinuxPlatformServices,
    shared_hhj_active: Arc<AtomicBool>,
    shell_state: &shell::ShellState,
) -> Option<floor_bounce_runtime::FloorBounceController> {
    platform.input_ready.then(|| {
        let input: Arc<dyn smu_input::InputDevice> = platform.input_service.clone();
        let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
        let foreground: Arc<dyn smu_process::ProcessForeground> =
            platform.process_foreground.clone();
        let control: Arc<dyn smu_process::ProcessControl> = platform.process_control.clone();
        floor_bounce_runtime::FloorBounceController::start(
            input,
            discovery,
            foreground,
            control,
            shared_hhj_active,
            shell_state.floor_bounce_controller_settings(),
        )
    })
}

#[cfg(target_os = "linux")]
fn start_hhj_controller(
    platform: &LinuxPlatformServices,
    shared_active: Arc<AtomicBool>,
    shell_state: &shell::ShellState,
) -> Option<hhj_runtime::HhjController> {
    platform.input_ready.then(|| {
        let input: Arc<dyn smu_input::InputDevice> = platform.input_service.clone();
        let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
        let foreground: Arc<dyn smu_process::ProcessForeground> =
            platform.process_foreground.clone();
        let control: Arc<dyn smu_process::ProcessControl> = platform.process_control.clone();
        hhj_runtime::HhjController::start(
            input,
            discovery,
            foreground,
            control,
            shared_active,
            shell_state.hhj_controller_settings(),
        )
    })
}

#[cfg(target_os = "linux")]
fn start_lag_switch_controller(
    platform: &LinuxPlatformServices,
    lag_switch_backend: &Arc<LinuxLagSwitchBackend>,
    lag_switch_enabled: &Arc<AtomicBool>,
    shell_state: &shell::ShellState,
) -> Option<lag_switch_runtime::LagSwitchController> {
    platform.input_ready.then(|| {
        let input: Arc<dyn smu_input::KeyState> = platform.input_service.clone();
        lag_switch_runtime::LagSwitchController::start(
            input,
            Arc::clone(lag_switch_backend),
            Arc::clone(lag_switch_enabled),
            shell_state.lag_switch_controller_settings(),
        )
    })
}

#[cfg(target_os = "linux")]
fn start_press_key_controller(
    input_ready: bool,
    input_service: &Arc<EvdevUinputInputDevice>,
    process_discovery: &Arc<ProcDiscovery>,
    process_foreground: &Arc<ProcForegroundProcess>,
    shell_state: &shell::ShellState,
) -> Option<press_key_runtime::PressKeyController> {
    input_ready.then(|| {
        let input: Arc<dyn smu_input::InputDevice> = input_service.clone();
        let discovery: Arc<dyn smu_process::ProcessDiscovery> = process_discovery.clone();
        let foreground: Arc<dyn smu_process::ProcessForeground> = process_foreground.clone();
        press_key_runtime::PressKeyController::start(
            input,
            discovery,
            foreground,
            shell_state.press_key_controller_settings(),
        )
    })
}

#[cfg(target_os = "linux")]
fn start_wallhop_controller(
    input_ready: bool,
    input_service: &Arc<EvdevUinputInputDevice>,
    process_discovery: &Arc<ProcDiscovery>,
    process_foreground: &Arc<ProcForegroundProcess>,
    shell_state: &shell::ShellState,
) -> Option<wallhop_runtime::WallhopController> {
    input_ready.then(|| {
        let input: Arc<dyn smu_input::InputDevice> = input_service.clone();
        let discovery: Arc<dyn smu_process::ProcessDiscovery> = process_discovery.clone();
        let foreground: Arc<dyn smu_process::ProcessForeground> = process_foreground.clone();
        wallhop_runtime::WallhopController::start(
            input,
            discovery,
            foreground,
            shell_state.wallhop_controller_settings(),
        )
    })
}

#[cfg(target_os = "linux")]
fn start_freeze_controller(
    platform: &LinuxPlatformServices,
    shell_state: &shell::ShellState,
) -> Option<freeze_runtime::FreezeController> {
    platform.input_ready.then(|| {
        let input: Arc<dyn smu_input::KeyState> = platform.input_service.clone();
        let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
        let foreground: Arc<dyn smu_process::ProcessForeground> =
            platform.process_foreground.clone();
        let control: Arc<dyn smu_process::ProcessControl> = platform.process_control.clone();
        freeze_runtime::FreezeController::start(
            input,
            discovery,
            foreground,
            control,
            shell_state.freeze_controller_settings(),
        )
    })
}

#[cfg(target_os = "linux")]
fn start_item_desync_controller(
    platform: &LinuxPlatformServices,
    shell_state: &shell::ShellState,
) -> Option<item_desync_runtime::ItemDesyncController> {
    platform.input_ready.then(|| {
        let input: Arc<dyn smu_input::InputDevice> = platform.input_service.clone();
        let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
        let foreground: Arc<dyn smu_process::ProcessForeground> =
            platform.process_foreground.clone();
        item_desync_runtime::ItemDesyncController::start(
            input,
            discovery,
            foreground,
            shell_state.item_desync_controller_settings(),
        )
    })
}

#[cfg(target_os = "linux")]
fn start_item_unequip_controller(
    platform: &LinuxPlatformServices,
    shell_state: &shell::ShellState,
) -> Option<item_unequip_runtime::ItemUnequipController> {
    platform.input_ready.then(|| {
        let input: Arc<dyn smu_input::InputDevice> = platform.input_service.clone();
        let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
        let foreground: Arc<dyn smu_process::ProcessForeground> =
            platform.process_foreground.clone();
        item_unequip_runtime::ItemUnequipController::start(
            input,
            discovery,
            foreground,
            shell_state.item_unequip_controller_settings(),
        )
    })
}

#[cfg(target_os = "linux")]
fn start_item_clip_controller(
    platform: &LinuxPlatformServices,
    shell_state: &shell::ShellState,
) -> Option<item_clip_runtime::ItemClipController> {
    platform.input_ready.then(|| {
        let input: Arc<dyn smu_input::InputDevice> = platform.input_service.clone();
        let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
        let foreground: Arc<dyn smu_process::ProcessForeground> =
            platform.process_foreground.clone();
        item_clip_runtime::ItemClipController::start(
            input,
            discovery,
            foreground,
            shell_state.item_clip_controller_settings(),
        )
    })
}

#[cfg(target_os = "linux")]
fn start_chat_macro_controllers(
    platform: &LinuxPlatformServices,
    shell_state: &shell::ShellState,
) -> (
    Option<item_unequip_runtime::ItemUnequipController>,
    Option<laugh_clip_runtime::LaughClipController>,
) {
    (
        start_item_unequip_controller(platform, shell_state),
        start_laugh_clip_controller(platform, shell_state),
    )
}

#[cfg(target_os = "linux")]
fn start_laugh_clip_controller(
    platform: &LinuxPlatformServices,
    shell_state: &shell::ShellState,
) -> Option<laugh_clip_runtime::LaughClipController> {
    platform.input_ready.then(|| {
        let input: Arc<dyn smu_input::InputDevice> = platform.input_service.clone();
        let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
        let foreground: Arc<dyn smu_process::ProcessForeground> =
            platform.process_foreground.clone();
        laugh_clip_runtime::LaughClipController::start(
            input,
            discovery,
            foreground,
            shell_state.laugh_clip_controller_settings(),
        )
    })
}

#[cfg(target_os = "linux")]
fn start_ledge_bounce_controller(
    platform: &LinuxPlatformServices,
    shell_state: &shell::ShellState,
) -> Option<ledge_bounce_runtime::LedgeBounceController> {
    platform.input_ready.then(|| {
        let input: Arc<dyn smu_input::InputDevice> = platform.input_service.clone();
        let discovery: Arc<dyn smu_process::ProcessDiscovery> = platform.process_discovery.clone();
        let foreground: Arc<dyn smu_process::ProcessForeground> =
            platform.process_foreground.clone();
        ledge_bounce_runtime::LedgeBounceController::start(
            input,
            discovery,
            foreground,
            shell_state.ledge_bounce_controller_settings(),
        )
    })
}

#[cfg(target_os = "linux")]
fn start_spam_key_controller(
    input_ready: bool,
    input_service: &Arc<EvdevUinputInputDevice>,
    process_discovery: &Arc<ProcDiscovery>,
    process_foreground: &Arc<ProcForegroundProcess>,
    shell_state: &shell::ShellState,
) -> Option<spam_key_runtime::SpamKeyController> {
    input_ready.then(|| {
        let input: Arc<dyn smu_input::InputDevice> = input_service.clone();
        let discovery: Arc<dyn smu_process::ProcessDiscovery> = process_discovery.clone();
        let foreground: Arc<dyn smu_process::ProcessForeground> = process_foreground.clone();
        spam_key_runtime::SpamKeyController::start(
            input,
            discovery,
            foreground,
            shell_state.spam_key_controller_settings(),
        )
    })
}

#[cfg(target_os = "linux")]
fn start_speedglitch_controller(
    input_ready: bool,
    input_service: &Arc<EvdevUinputInputDevice>,
    process_discovery: &Arc<ProcDiscovery>,
    process_foreground: &Arc<ProcForegroundProcess>,
    shell_state: &shell::ShellState,
) -> Option<speedglitch_runtime::SpeedglitchController> {
    input_ready.then(|| {
        let input: Arc<dyn smu_input::InputDevice> = input_service.clone();
        let discovery: Arc<dyn smu_process::ProcessDiscovery> = process_discovery.clone();
        let foreground: Arc<dyn smu_process::ProcessForeground> = process_foreground.clone();
        speedglitch_runtime::SpeedglitchController::start(
            input,
            discovery,
            foreground,
            shell_state.speedglitch_controller_settings(),
        )
    })
}

#[cfg(target_os = "linux")]
fn start_wall_walk_controller(
    input_ready: bool,
    input_service: &Arc<EvdevUinputInputDevice>,
    process_discovery: &Arc<ProcDiscovery>,
    process_foreground: &Arc<ProcForegroundProcess>,
    shell_state: &shell::ShellState,
) -> Option<wall_walk_runtime::WallWalkController> {
    input_ready.then(|| {
        let input: Arc<dyn smu_input::InputDevice> = input_service.clone();
        let discovery: Arc<dyn smu_process::ProcessDiscovery> = process_discovery.clone();
        let foreground: Arc<dyn smu_process::ProcessForeground> = process_foreground.clone();
        wall_walk_runtime::WallWalkController::start(
            input,
            discovery,
            foreground,
            shell_state.wall_walk_controller_settings(),
        )
    })
}

#[cfg(target_os = "linux")]
fn start_walless_lhj_controller(
    input_ready: bool,
    input_service: &Arc<EvdevUinputInputDevice>,
    process_discovery: &Arc<ProcDiscovery>,
    process_foreground: &Arc<ProcForegroundProcess>,
    process_control: &Arc<ProcCgroupProcessControl>,
    shell_state: &shell::ShellState,
) -> Option<walless_lhj_runtime::WallessLhjController> {
    input_ready.then(|| {
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
    })
}

#[cfg(target_os = "linux")]
impl LinuxRuntimeServices {
    fn new(shell_state: &shell::ShellState) -> Self {
        let roblox_log: Arc<dyn RobloxLogSource> = Arc::new(SharedRobloxLogReader::default());
        let lag_switch_backend = Arc::new(LinuxLagSwitchBackend::new(Some(roblox_log)));
        let lag_switch_enabled = Arc::new(AtomicBool::new(false));
        let (freeze_authorization, platform) = create_linux_platform_services();
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
        let press_key_controller = start_press_key_controller(
            platform.input_ready,
            &platform.input_service,
            &platform.process_discovery,
            &platform.process_foreground,
            shell_state,
        );
        let spam_key_controller = start_spam_key_controller(
            platform.input_ready,
            &platform.input_service,
            &platform.process_discovery,
            &platform.process_foreground,
            shell_state,
        );
        let ledge_bounce_controller = start_ledge_bounce_controller(&platform, shell_state);
        let speedglitch_controller = start_speedglitch_controller(
            platform.input_ready,
            &platform.input_service,
            &platform.process_discovery,
            &platform.process_foreground,
            shell_state,
        );
        let wall_walk_controller = start_wall_walk_controller(
            platform.input_ready,
            &platform.input_service,
            &platform.process_discovery,
            &platform.process_foreground,
            shell_state,
        );
        let walless_lhj_controller = start_walless_lhj_controller(
            platform.input_ready,
            &platform.input_service,
            &platform.process_discovery,
            &platform.process_foreground,
            &platform.process_control,
            shell_state,
        );
        let wallhop_controller = start_wallhop_controller(
            platform.input_ready,
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
            _display_service: platform.display_service,
            wayland_portal: platform.wayland_portal,
            wayland_settings_busy: Arc::new(AtomicBool::new(false)),
            wayland_settings_worker: Mutex::new(None),
            input_service: platform.input_service,
            lag_switch_controller,
            bunnyhop_controller,
            hhj_controller,
            floor_bounce_controller,
            freeze_controller,
            item_desync_controller,
            item_clip_controller,
            item_unequip_controller,
            laugh_clip_controller,
            press_key_controller,
            spam_key_controller,
            ledge_bounce_controller,
            speedglitch_controller,
            wall_walk_controller,
            walless_lhj_controller,
            wallhop_controller,
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
        if shell_state.keybind_input_poll_needed() {
            let input = keybind::InputSnapshot::capture(
                self.input_service.as_ref(),
                shell_state.keybind_capture_active(),
            );
            shell_state.set_keybind_input(input);
        }
        let selector = shell_state.process_selector();
        shell_state.set_process_found(self.process_found(&selector));
        shell_state.set_settings_platform_status(self.settings_platform_status());

        let available = self.lag_switch_backend.is_available();
        shell_state.set_lag_switch_backend(lag_switch::BackendState {
            enabled: self.lag_switch_enabled.load(Ordering::Acquire),
            available,
            actively_lagging: self.lag_switch_backend.is_blocking_active(),
        });
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
        action: Option<shell::ShellAction>,
        shell_state: &mut shell::ShellState,
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
        if let Some(controller) = self.lag_switch_controller.as_ref() {
            controller.update_settings(shell_state.lag_switch_controller_settings());
            if controller.take_helper_prompt_request() {
                shell_state
                    .set_lag_switch_helper_error(self.lag_switch_backend.unsupported_reason());
                shell_state.request_lag_switch_helper_popup();
            }
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
        self.handle_action(action, shell_state);
    }

    fn handle_action(
        &self,
        action: Option<shell::ShellAction>,
        shell_state: &mut shell::ShellState,
    ) {
        match action {
            Some(shell::ShellAction::ToggleLagSwitchBackend) => {
                if self.lag_switch_enabled.load(Ordering::Acquire) {
                    self.lag_switch_backend.shutdown();
                    self.lag_switch_enabled.store(false, Ordering::Release);
                } else {
                    let config = shell_state.lag_switch_controller_settings().config;
                    self.lag_switch_backend.set_base_config(config);
                    if self.lag_switch_backend.init() {
                        self.lag_switch_enabled.store(true, Ordering::Release);
                    } else {
                        shell_state.set_lag_switch_helper_error(String::new());
                        shell_state.request_lag_switch_helper_popup();
                    }
                }
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
                        shell_state.request_lag_switch_helper_popup();
                    }
                    Err(error) => {
                        shell_state.set_lag_switch_helper_error(error);
                        shell_state.request_lag_switch_helper_popup();
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
            None => {}
        }
    }

    fn shutdown(&self) {
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
        self.input_service.shutdown();
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdl = sdl3::init()?;
    let video = sdl.video()?;
    let mut events = sdl.event_pump()?;

    enable_native_ime_ui();
    configure_gl_attributes(&video);

    let window_width = capture_dimension("SMU_UI_CAPTURE_WIDTH", MINIMUM_WIDTH);
    let window_height = capture_dimension("SMU_UI_CAPTURE_HEIGHT", MINIMUM_HEIGHT);
    let mut window =
        video.window(WINDOW_TITLE, window_width, window_height).opengl().resizable().build()?;
    window.set_minimum_size(MINIMUM_WIDTH, MINIMUM_HEIGHT)?;
    let gl_context = window.gl_create_context()?;
    window.gl_make_current(&gl_context)?;
    gl::load_with(|symbol| {
        video
            .gl_get_proc_address(symbol)
            .map_or(ptr::null(), |function| (function as *const ()).cast::<c_void>())
    });

    let mut imgui = Context::create();
    imgui.set_ini_filename::<PathBuf>(None)?;
    theme::configure(&mut imgui)?;

    // SAFETY: `window` and `gl_context` are declared before `imgui`/backend and therefore outlive
    // the native Dear ImGui attachments that retain their pointers.
    let mut backend =
        unsafe { Sdl3OpenGl3Backend::init(&mut imgui, &window, &gl_context, GLSL_VERSION)? };
    let parity_case = std::env::var("SMU_UI_PARITY_CASE").ok();
    let capture_path = std::env::var_os("SMU_UI_CAPTURE_PATH").map(PathBuf::from);
    let mut shell_state = shell::ShellState::for_parity_case(parity_case.as_deref());
    #[cfg(target_os = "linux")]
    let linux_services = LinuxRuntimeServices::new(&shell_state);
    let mut rendered_frames = 0_u32;

    'main: loop {
        for event in events.poll_iter() {
            backend.process_event(&mut imgui, &event)?;
            let should_quit = matches!(event, Event::Quit { .. })
                || (matches!(event, Event::KeyDown { keycode: Some(Keycode::Escape), .. })
                    && !shell_state.keybind_capture_active());
            if should_quit {
                break 'main;
            }
        }

        backend.new_frame(&mut imgui)?;
        let frame = imgui.begin_frame();
        let ui = frame.ui();
        #[cfg(target_os = "linux")]
        linux_services.prepare_shell(&mut shell_state);
        let shell_action = shell::render(ui, &mut shell_state);
        #[cfg(target_os = "linux")]
        linux_services.finish_shell(shell_action, &mut shell_state);
        #[cfg(not(target_os = "linux"))]
        let _ = shell_action;

        let prepared = backend.prepare(frame, || window.gl_make_current(&gl_context))?;

        // Match the current C++ renderer's clear color. The root ImGui window normally covers the
        // full client area, but keeping the same clear path avoids edge artifacts during parity
        // captures and makes framebuffer state deterministic.
        // SAFETY: the SDL OpenGL context is current on this thread and the loaded function pointers
        // come from that context's SDL video subsystem.
        unsafe {
            gl::ClearColor(0.08, 0.09, 0.10, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }
        backend.render_main(prepared)?;
        rendered_frames = rendered_frames.saturating_add(1);

        if let Some(path) = capture_path.as_deref()
            && rendered_frames >= 3
        {
            capture::save_back_buffer(&window, path)?;
            break 'main;
        }

        window.gl_swap_window();
    }

    #[cfg(target_os = "linux")]
    linux_services.shutdown();

    Ok(())
}
