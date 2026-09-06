use crate::{
    InputInjector, KeyState, windows_is_extended_virtual_key, windows_is_mouse_button_virtual_key,
    windows_modifier_scan_code, windows_scale_relative_delta, windows_virtual_key,
};
use smu_core::KeyCode;
use std::{
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicI32, AtomicIsize, Ordering},
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::UI::{
    Input::KeyboardAndMouse::{
        GetAsyncKeyState, GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE,
        KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MAPVK_VK_TO_VSC_EX,
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
        MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL,
        MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, MapVirtualKeyExW, SendInput,
    },
    WindowsAndMessaging::{
        GA_ROOT, GetAncestor, GetForegroundWindow, GetWindowThreadProcessId, XBUTTON1, XBUTTON2,
    },
};

const INJECTED_INPUT_TAG: usize = 0x534D_4301;
const GUI_INPUT_BUDGET: usize = 50;
const GUI_INPUT_BUDGET_SLICE: Duration = Duration::from_millis(16);
static DISPLAY_SCALE_PERCENT: AtomicI32 = AtomicI32::new(100);
static GUI_WINDOW_HANDLE: AtomicIsize = AtomicIsize::new(0);
static INPUT_BUDGET_STATE: OnceLock<Mutex<InputBudgetState>> = OnceLock::new();

struct InputBudgetState {
    reset_time: Instant,
    remaining: usize,
}

pub fn set_display_scale_percent(scale: i32) {
    DISPLAY_SCALE_PERCENT.store(scale.clamp(10, 500), Ordering::Release);
}

pub fn set_gui_window_handle(handle: *mut core::ffi::c_void) {
    GUI_WINDOW_HANDLE.store(handle as isize, Ordering::Release);
}

fn gui_window_is_foreground() -> bool {
    let raw = GUI_WINDOW_HANDLE.load(Ordering::Acquire);
    if raw == 0 {
        return false;
    }
    let gui = raw as *mut core::ffi::c_void;
    // SAFETY: both calls read window-manager state only. `gui` is the HWND SDL provided for the live
    // application window and is never dereferenced by Rust.
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_null() {
        return false;
    }
    foreground == gui || unsafe { GetAncestor(foreground, GA_ROOT) } == gui
}

fn acquire_input_budget(input_count: usize) {
    if !gui_window_is_foreground() {
        return;
    }
    let state = INPUT_BUDGET_STATE.get_or_init(|| {
        Mutex::new(InputBudgetState { reset_time: Instant::now(), remaining: GUI_INPUT_BUDGET })
    });
    loop {
        let sleep_for = {
            let mut state = match state.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            let now = Instant::now();
            let elapsed = now.saturating_duration_since(state.reset_time);
            if elapsed >= GUI_INPUT_BUDGET_SLICE {
                state.reset_time = now;
                state.remaining = GUI_INPUT_BUDGET;
            }
            if input_count <= state.remaining {
                state.remaining -= input_count;
                return;
            }
            GUI_INPUT_BUDGET_SLICE.saturating_sub(elapsed).max(Duration::from_millis(1))
        };
        std::thread::sleep(sleep_for);
    }
}

fn scale_relative_delta(delta: i32) -> i32 {
    windows_scale_relative_delta(delta, DISPLAY_SCALE_PERCENT.load(Ordering::Acquire))
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsKeyState;

impl WindowsKeyState {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

fn foreground_keyboard_layout() -> *mut core::ffi::c_void {
    // SAFETY: these Win32 calls retain no caller-owned pointers. A null foreground window simply
    // falls back to this thread's active keyboard layout, matching the C++ backend.
    unsafe {
        let foreground = GetForegroundWindow();
        if !foreground.is_null() {
            let thread = GetWindowThreadProcessId(foreground, std::ptr::null_mut());
            if thread != 0 {
                let layout = GetKeyboardLayout(thread);
                if !layout.is_null() {
                    return layout;
                }
            }
        }
        GetKeyboardLayout(0)
    }
}

fn scan_code_for_virtual_key(vk: u16) -> u16 {
    if let Some(scan_code) = windows_modifier_scan_code(vk) {
        return scan_code;
    }
    // SAFETY: the keyboard-layout handle comes from Win32 and the function has no pointer
    // arguments. The low byte matches C++ `ScanCodeForVirtualKey`.
    unsafe {
        (MapVirtualKeyExW(u32::from(vk), MAPVK_VK_TO_VSC_EX, foreground_keyboard_layout()) & 0xFF)
            as u16
    }
}

fn keyboard_input(scan_code: u16, extended: bool, key_up: bool) -> INPUT {
    let mut flags = KEYEVENTF_SCANCODE;
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: scan_code,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECTED_INPUT_TAG,
            },
        },
    }
}

fn mouse_input(flags: u32, mouse_data: u32, dx: i32, dy: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: mouse_data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECTED_INPUT_TAG,
            },
        },
    }
}

fn dispatch(input: &INPUT) {
    acquire_input_budget(1);
    // SAFETY: `input` points to one initialized Win32 INPUT record and remains valid for the call.
    unsafe {
        let _ =
            SendInput(1, input, i32::try_from(std::mem::size_of::<INPUT>()).unwrap_or_default());
    }
}

fn mouse_button_input(vk: u16, down: bool) -> INPUT {
    let (flags, data) = match vk {
        0x01 => (if down { MOUSEEVENTF_LEFTDOWN } else { MOUSEEVENTF_LEFTUP }, 0),
        0x02 => (if down { MOUSEEVENTF_RIGHTDOWN } else { MOUSEEVENTF_RIGHTUP }, 0),
        0x04 => (if down { MOUSEEVENTF_MIDDLEDOWN } else { MOUSEEVENTF_MIDDLEUP }, 0),
        0x05 => (if down { MOUSEEVENTF_XDOWN } else { MOUSEEVENTF_XUP }, u32::from(XBUTTON1)),
        0x06 => (if down { MOUSEEVENTF_XDOWN } else { MOUSEEVENTF_XUP }, u32::from(XBUTTON2)),
        // C++ treats the full VK_LBUTTON..VK_XBUTTON2 range as mouse input, including VK_CANCEL.
        _ => (0, 0),
    };
    mouse_input(flags, data, 0, 0)
}

fn inject_virtual_key(key: KeyCode, extended: bool, down: bool) {
    let vk = key.raw() as u16;
    if windows_is_mouse_button_virtual_key(vk) {
        dispatch(&mouse_button_input(vk, down));
        return;
    }
    let scan_code = scan_code_for_virtual_key(vk);
    if scan_code == 0 {
        return;
    }
    dispatch(&keyboard_input(scan_code, extended || windows_is_extended_virtual_key(vk), !down));
}

impl KeyState for WindowsKeyState {
    fn is_pressed(&self, key: KeyCode) -> bool {
        let Some(vkey) = windows_virtual_key(key) else {
            return false;
        };
        // SAFETY: GetAsyncKeyState accepts a Win32 virtual-key code and has no pointer arguments.
        (unsafe { GetAsyncKeyState(vkey) } as u16 & 0x8000) != 0
    }

    fn current_pressed_key(&self) -> Option<KeyCode> {
        (1_u32..KeyCode::MOUSE_WHEEL_UP.raw())
            .find(|&raw| self.is_pressed(KeyCode::new(raw)))
            .map(KeyCode::new)
    }
}

impl InputInjector for WindowsKeyState {
    fn hold_key(&self, key: KeyCode, extended: bool) {
        inject_virtual_key(key, extended, true);
    }

    fn release_key(&self, key: KeyCode, extended: bool) {
        inject_virtual_key(key, extended, false);
    }

    fn move_mouse(&self, dx: i32, dy: i32) {
        self.move_mouse_raw(scale_relative_delta(dx), scale_relative_delta(dy));
    }

    fn move_mouse_raw(&self, dx: i32, dy: i32) {
        dispatch(&mouse_input(MOUSEEVENTF_MOVE, 0, dx, dy));
    }

    fn mouse_wheel(&self, delta: i32) {
        dispatch(&mouse_input(MOUSEEVENTF_WHEEL, delta as u32, 0, 0));
    }
}
