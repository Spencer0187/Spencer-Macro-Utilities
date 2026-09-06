use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::UI::{
    Shell::{SHELLEXECUTEINFOW, ShellExecuteExW},
    WindowsAndMessaging::SW_SHOWNORMAL,
};

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

pub fn restart_as_admin() -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("Could not resolve the current executable: {error}"))?;
    let verb = wide_null(OsStr::new("runas"));
    let executable = wide_null(executable.as_os_str());
    let mut info = SHELLEXECUTEINFOW {
        cbSize: u32::try_from(std::mem::size_of::<SHELLEXECUTEINFOW>()).unwrap_or_default(),
        fMask: 0,
        hwnd: std::ptr::null_mut(),
        lpVerb: verb.as_ptr(),
        lpFile: executable.as_ptr(),
        lpParameters: std::ptr::null(),
        lpDirectory: std::ptr::null(),
        nShow: SW_SHOWNORMAL,
        hInstApp: std::ptr::null_mut(),
        lpIDList: std::ptr::null_mut(),
        lpClass: std::ptr::null(),
        hkeyClass: std::ptr::null_mut(),
        dwHotKey: 0,
        Anonymous: Default::default(),
        hProcess: std::ptr::null_mut(),
    };
    // SAFETY: all string buffers remain alive for the synchronous ShellExecuteExW call and the
    // structure contains either null handles/pointers or pointers into those live buffers.
    if unsafe { ShellExecuteExW(&raw mut info) } == 0 {
        return Err(format!(
            "Could not restart SMU as Administrator: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_null_adds_one_terminator() {
        let encoded = wide_null(OsStr::new("runas"));
        assert_eq!(encoded.last(), Some(&0));
        assert_eq!(encoded.iter().filter(|&&unit| unit == 0).count(), 1);
    }
}
