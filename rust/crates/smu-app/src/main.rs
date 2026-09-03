//! Minimal `SDL3` + Dear `ImGui` + `OpenGL3` compatibility spike.

use std::path::PathBuf;

use dear_imgui_rs::{Condition, Context};
use dear_imgui_sdl3::{Sdl3OpenGl3Backend, enable_native_ime_ui};
use sdl3::{event::Event, keyboard::Keycode, video::GLProfile};

const WINDOW_TITLE: &str = "SMU Rust UI parity spike";
const INITIAL_WIDTH: u32 = 1180;
const INITIAL_HEIGHT: u32 = 700;
const INITIAL_UI_SIZE: [f32; 2] = [1180.0, 700.0];
const GLSL_VERSION: &str = "#version 150";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdl = sdl3::init()?;
    let video = sdl.video()?;
    let mut events = sdl.event_pump()?;

    enable_native_ime_ui();
    {
        let attributes = video.gl_attr();
        attributes.set_context_profile(GLProfile::Core);
        attributes.set_context_version(3, 3);
    }

    let window =
        video.window(WINDOW_TITLE, INITIAL_WIDTH, INITIAL_HEIGHT).opengl().resizable().build()?;
    let gl_context = window.gl_create_context()?;
    window.gl_make_current(&gl_context)?;

    let mut imgui = Context::create();
    imgui.set_ini_filename::<PathBuf>(None)?;

    // SAFETY: `window` and `gl_context` are declared before `imgui`/backend and therefore outlive
    // the native Dear ImGui attachments that retain their pointers.
    let mut backend =
        unsafe { Sdl3OpenGl3Backend::init(&mut imgui, &window, &gl_context, GLSL_VERSION)? };

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
        ui.window("Rust parity spike").size(INITIAL_UI_SIZE, Condition::FirstUseEver).build(|| {
            ui.text("Renderer path established; real SMU UI slices come next.");
        });

        let prepared = backend.prepare(frame, || window.gl_make_current(&gl_context))?;
        backend.render_main(prepared)?;
        window.gl_swap_window();
    }

    Ok(())
}
