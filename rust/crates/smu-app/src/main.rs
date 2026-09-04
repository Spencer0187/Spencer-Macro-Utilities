//! Minimal `SDL3` + Dear `ImGui` + `OpenGL3` compatibility spike.

use std::{ffi::c_void, path::PathBuf, ptr};

#[cfg(target_os = "linux")]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

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
use smu_process::linux::{
    ProcCgroupProcessControl, ProcDiscovery, ProcForegroundProcess, UnixFreezeHelper,
};
#[cfg(target_os = "linux")]
use smu_roblox::{RobloxLogSource, SharedRobloxLogReader};

mod capture;
mod freeze;
#[cfg(target_os = "linux")]
mod freeze_runtime;
mod global_settings;
mod item_desync;
#[cfg(target_os = "linux")]
mod item_desync_runtime;
mod lag_switch;
#[cfg(target_os = "linux")]
mod lag_switch_runtime;
#[cfg(target_os = "linux")]
mod linux_freeze_authorization;
#[cfg(target_os = "linux")]
mod linux_privileged_helper;
mod parity;
#[cfg(target_os = "linux")]
mod runtime_input;
mod shell;
mod sidebar;
#[cfg(target_os = "linux")]
mod target_runtime;
mod theme;

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
struct LinuxRuntimeServices {
    lag_switch_backend: Arc<LinuxLagSwitchBackend>,
    lag_switch_enabled: Arc<AtomicBool>,
    freeze_authorization: Arc<linux_freeze_authorization::FreezeHelperAuthorizationGate>,
    _process_control: Arc<ProcCgroupProcessControl>,
    _display_service: Arc<LinuxDisplayCapture>,
    wayland_portal: Arc<WaylandPortalCapture>,
    input_service: Arc<EvdevUinputInputDevice>,
    lag_switch_controller: Option<lag_switch_runtime::LagSwitchController>,
    freeze_controller: Option<freeze_runtime::FreezeController>,
    item_desync_controller: Option<item_desync_runtime::ItemDesyncController>,
}

#[cfg(target_os = "linux")]
impl LinuxRuntimeServices {
    fn new(shell_state: &shell::ShellState) -> Self {
        let roblox_log: Arc<dyn RobloxLogSource> = Arc::new(SharedRobloxLogReader::default());
        let lag_switch_backend = Arc::new(LinuxLagSwitchBackend::new(Some(roblox_log)));
        let lag_switch_enabled = Arc::new(AtomicBool::new(false));
        let freeze_authorization =
            Arc::new(linux_freeze_authorization::FreezeHelperAuthorizationGate::default());
        let process_control = Arc::new(
            ProcCgroupProcessControl::default()
                .with_helper(Arc::new(UnixFreezeHelper::default()), freeze_authorization.clone()),
        );
        let process_discovery = Arc::new(ProcDiscovery::default());
        let process_foreground = Arc::new(ProcForegroundProcess::default());
        let display_service = Arc::new(LinuxDisplayCapture::new());
        let wayland_portal = display_service.wayland_portal();
        let input_service =
            Arc::new(EvdevUinputInputDevice::new().with_wayland_portal(wayland_portal.clone()));
        let input_ready = input_service.init().is_ok();
        let lag_switch_controller = input_ready.then(|| {
            let input: Arc<dyn smu_input::KeyState> = input_service.clone();
            lag_switch_runtime::LagSwitchController::start(
                input,
                lag_switch_backend.clone(),
                lag_switch_enabled.clone(),
                shell_state.lag_switch_controller_settings(),
            )
        });
        let freeze_controller = input_ready.then(|| {
            let input: Arc<dyn smu_input::KeyState> = input_service.clone();
            let discovery: Arc<dyn smu_process::ProcessDiscovery> = process_discovery.clone();
            let foreground: Arc<dyn smu_process::ProcessForeground> = process_foreground.clone();
            let control: Arc<dyn smu_process::ProcessControl> = process_control.clone();
            freeze_runtime::FreezeController::start(
                input,
                discovery,
                foreground,
                control,
                shell_state.freeze_controller_settings(),
            )
        });
        let item_desync_controller = input_ready.then(|| {
            let input: Arc<dyn smu_input::InputDevice> = input_service.clone();
            let discovery: Arc<dyn smu_process::ProcessDiscovery> = process_discovery.clone();
            let foreground: Arc<dyn smu_process::ProcessForeground> = process_foreground.clone();
            item_desync_runtime::ItemDesyncController::start(
                input,
                discovery,
                foreground,
                shell_state.item_desync_controller_settings(),
            )
        });
        Self {
            lag_switch_backend,
            lag_switch_enabled,
            freeze_authorization,
            _process_control: process_control,
            _display_service: display_service,
            wayland_portal,
            input_service,
            lag_switch_controller,
            freeze_controller,
            item_desync_controller,
        }
    }

    fn prepare_shell(&self, shell_state: &mut shell::ShellState) {
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
            if matches!(
                event,
                Event::Quit { .. } | Event::KeyDown { keycode: Some(Keycode::Escape), .. }
            ) {
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
