//! Minimal `SDL3` + Dear `ImGui` + `OpenGL3` compatibility spike.

use std::{ffi::c_void, path::PathBuf, ptr};

#[cfg(target_os = "linux")]
use std::sync::Arc;

use dear_imgui_rs::Context;
use dear_imgui_sdl3::{Sdl3OpenGl3Backend, enable_native_ime_ui};
use sdl3::{event::Event, keyboard::Keycode, video::GLProfile};

#[cfg(target_os = "linux")]
use smu_network::{LagSwitchControl, linux::LinuxLagSwitchBackend};
#[cfg(target_os = "linux")]
use smu_roblox::{RobloxLogSource, SharedRobloxLogReader};

mod capture;
mod global_settings;
mod lag_switch;
#[cfg(target_os = "linux")]
mod linux_privileged_helper;
mod parity;
mod shell;
mod sidebar;
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

#[cfg(target_os = "linux")]
fn sync_linux_lag_switch_status(
    backend: &LinuxLagSwitchBackend,
    shell_state: &mut shell::ShellState,
) {
    let available = backend.is_available();
    shell_state.set_lag_switch_backend(lag_switch::BackendState {
        enabled: available,
        available,
        actively_lagging: backend.is_blocking_active(),
    });
}

#[cfg(target_os = "linux")]
fn handle_linux_shell_action(
    action: Option<shell::ShellAction>,
    backend: &LinuxLagSwitchBackend,
    shell_state: &mut shell::ShellState,
) {
    match action {
        Some(shell::ShellAction::ToggleLagSwitchBackend) => {
            if backend.is_available() {
                backend.shutdown();
            } else if !backend.init() {
                shell_state.set_lag_switch_helper_error(String::new());
                shell_state.request_lag_switch_helper_popup();
            }
        }
        Some(shell::ShellAction::StartLagSwitchHelper) => {
            match linux_privileged_helper::start_linux_network_helper_with_graphical_pkexec() {
                Ok(()) if backend.init() => {
                    shell_state.set_lag_switch_helper_error(String::new());
                }
                Ok(()) => {
                    shell_state.set_lag_switch_helper_error(backend.unsupported_reason());
                    shell_state.request_lag_switch_helper_popup();
                }
                Err(error) => {
                    shell_state.set_lag_switch_helper_error(error);
                    shell_state.request_lag_switch_helper_popup();
                }
            }
        }
        None => {}
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdl = sdl3::init()?;
    let video = sdl.video()?;
    let mut events = sdl.event_pump()?;

    enable_native_ime_ui();
    {
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
    let lag_switch_backend = {
        let roblox_log: Arc<dyn RobloxLogSource> = Arc::new(SharedRobloxLogReader::default());
        LinuxLagSwitchBackend::new(Some(roblox_log))
    };
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
        sync_linux_lag_switch_status(&lag_switch_backend, &mut shell_state);
        let shell_action = shell::render(ui, &mut shell_state);
        #[cfg(target_os = "linux")]
        handle_linux_shell_action(shell_action, &lag_switch_backend, &mut shell_state);
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

    Ok(())
}
