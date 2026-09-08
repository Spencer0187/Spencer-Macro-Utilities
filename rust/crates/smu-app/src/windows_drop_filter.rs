//! Windows elevated-process drag-and-drop compatibility.
//!
//! Explorer runs unelevated by default. When SMU is elevated for WinDivert, UIPI can therefore block SDL's
//! normal file-drop messages unless the live window explicitly allows the same three messages as the C++ app.

use std::{ffi::c_void, mem::transmute, ptr::null_mut};
use windows_sys::Win32::{
    Foundation::HWND,
    System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
    UI::WindowsAndMessaging::{MSGFLT_ALLOW, WM_COPYDATA, WM_DROPFILES},
};

const WM_COPYGLOBALDATA: u32 = 0x0049;

type ChangeWindowMessageFilterExFn = unsafe extern "system" fn(HWND, u32, u32, *mut c_void) -> i32;

/// Allow Explorer-to-elevated-SMU drag/drop messages on the current SDL Win32 window.
///
/// Returns warning strings so the app can route them through its shared logger/toast path without coupling this
/// Win32-only helper to the scripting crate.
pub fn allow_elevated_file_drops(hwnd: HWND) -> Vec<String> {
    let mut warnings = Vec::new();
    if hwnd.is_null() {
        warnings.push(
            "Could not find Win32 window handle for drag-and-drop message filter.".to_owned(),
        );
        return warnings;
    }

    let user32 = "user32.dll\0".encode_utf16().collect::<Vec<_>>();
    // SAFETY: `user32` is a valid NUL-terminated UTF-16 module name.
    let module = unsafe { GetModuleHandleW(user32.as_ptr()) };
    if module.is_null() {
        warnings.push(
            "ChangeWindowMessageFilterEx is unavailable; elevated Windows drag-and-drop may be blocked."
                .to_owned(),
        );
        return warnings;
    }

    // SAFETY: the proc name is a valid static NUL-terminated ANSI string and `module` is a loaded user32 module.
    let Some(raw_proc) =
        (unsafe { GetProcAddress(module, c"ChangeWindowMessageFilterEx".as_ptr().cast()) })
    else {
        warnings.push(
            "ChangeWindowMessageFilterEx is unavailable; elevated Windows drag-and-drop may be blocked."
                .to_owned(),
        );
        return warnings;
    };
    // SAFETY: the symbol is resolved by its exact Win32 API name from user32.dll, whose documented signature is
    // BOOL ChangeWindowMessageFilterEx(HWND, UINT, DWORD, PCHANGEFILTERSTRUCT).
    let change_message_filter = unsafe {
        transmute::<unsafe extern "system" fn() -> isize, ChangeWindowMessageFilterExFn>(raw_proc)
    };

    for message in [WM_DROPFILES, WM_COPYDATA, WM_COPYGLOBALDATA] {
        // SAFETY: `hwnd` is the current live SDL window; null CHANGEFILTERSTRUCT is explicitly accepted by this API.
        let allowed = unsafe { change_message_filter(hwnd, message, MSGFLT_ALLOW, null_mut()) };
        if allowed == 0 {
            warnings
                .push(format!("Could not allow elevated Windows drag-and-drop message {message}."));
        }
    }
    warnings
}
