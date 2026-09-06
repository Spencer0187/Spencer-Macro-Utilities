use crate::{KeyState, windows_virtual_key};
use smu_core::KeyCode;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsKeyState;

impl WindowsKeyState {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl KeyState for WindowsKeyState {
    fn is_pressed(&self, key: KeyCode) -> bool {
        let Some(vkey) = windows_virtual_key(key) else {
            return false;
        };
        // SAFETY: GetAsyncKeyState accepts a Win32 virtual-key code and has no pointer arguments.
        (unsafe { GetAsyncKeyState(vkey) } as u16 & 0x8000) != 0
    }
}
