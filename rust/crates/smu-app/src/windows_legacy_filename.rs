use std::path::{Path, PathBuf};

const VERSION_BYTES: &[u8] = include_bytes!("../../../../version");
const LEGACY_FILENAME: &str = "suspend.exe";
const V340_TARGET_FILENAME: &str = "Spencer-Macro-Utilities-V3.4.0-Windows.exe";
const MIGRATION_TITLE: &str = "Spencer Macro Utilities filename migration";

// V3.4.0-ONLY LEGACY BRIDGE.
//
// This intentionally fails compilation as soon as the repository version changes. Delete this module and its
// startup call before V3.4.1 or later, matching the C++ static_assert tripwire.
const _: () = {
    // Git checkouts may use LF or CRLF depending on the host's autocrlf setting.
    // Keep the exact 3.4.0-only migration tripwire without depending on that setting.
    assert!(VERSION_BYTES.len() == 6 || VERSION_BYTES.len() == 7);
    assert!(VERSION_BYTES[0] == b'3');
    assert!(VERSION_BYTES[1] == b'.');
    assert!(VERSION_BYTES[2] == b'4');
    assert!(VERSION_BYTES[3] == b'.');
    assert!(VERSION_BYTES[4] == b'0');
    assert!(if VERSION_BYTES.len() == 6 {
        VERSION_BYTES[5] == b'\n'
    } else {
        VERSION_BYTES[5] == b'\r' && VERSION_BYTES[6] == b'\n'
    });
};

#[must_use]
fn is_legacy_filename(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(LEGACY_FILENAME))
}

#[must_use]
fn migration_target_path(current_path: &Path) -> PathBuf {
    current_path.parent().unwrap_or_else(|| Path::new(".")).join(V340_TARGET_FILENAME)
}

const MIGRATION_SCRIPT: &str = concat!(
    "@echo off\r\n",
    "setlocal\r\n",
    "set \"SMU_OLD=%~1\"\r\n",
    "set \"SMU_NEW=%~2\"\r\n",
    "set \"SMU_SYSTEM=%~3\"\r\n",
    "set /A SMU_DELETE_ATTEMPTS=0\r\n",
    ":wait_for_old_image\r\n",
    "del /F /Q \"%SMU_OLD%\" > NUL 2>&1\r\n",
    "if not exist \"%SMU_OLD%\" goto launch_new\r\n",
    "set /A SMU_DELETE_ATTEMPTS+=1\r\n",
    "if %SMU_DELETE_ATTEMPTS% GEQ 300 goto launch_new\r\n",
    "\"%SMU_SYSTEM%\\timeout.exe\" /t 1 /nobreak > NUL\r\n",
    "goto wait_for_old_image\r\n",
    ":launch_new\r\n",
    "start \"\" \"%SMU_NEW%\"\r\n",
    "endlocal\r\n",
    "set \"SMU_HELPER=%~f0\"\r\n",
    "set \"SMU_SYSTEM=%~3\"\r\n",
    "start \"\" /B \"%SMU_SYSTEM%\\WindowsPowerShell\\v1.0\\powershell.exe\" -NoProfile -NonInteractive -WindowStyle Hidden -Command \"Start-Sleep -Milliseconds 500; Remove-Item -LiteralPath $env:SMU_HELPER -Force\"\r\n",
    "exit /B\r\n",
);

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::ffi::OsStr;
    use std::mem::size_of;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CREATE_NEW, CopyFileW, CreateFileW, DeleteFileW, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_TEMPORARY, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, FlushFileBuffers, GetFileAttributesW, GetFileSizeEx,
        INVALID_FILE_ATTRIBUTES, OPEN_EXISTING, ReadFile, WriteFile,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW;
    use windows_sys::Win32::System::SystemInformation::{GetSystemDirectoryW, GetTickCount64};
    use windows_sys::Win32::System::Threading::{
        CREATE_NO_WINDOW, CreateProcessW, GetCurrentProcessId, PROCESS_INFORMATION, STARTUPINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONWARNING, MB_OK, MessageBoxW};

    const MAX_EXECUTABLE_PATH: usize = 32_768;
    const MAX_SYSTEM_PATH: usize = 260;

    struct OwnedHandle(HANDLE);

    impl OwnedHandle {
        fn from_raw(handle: HANDLE) -> Option<Self> {
            (handle != INVALID_HANDLE_VALUE && !handle.is_null()).then_some(Self(handle))
        }
    }

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            // SAFETY: OwnedHandle is constructed only from successful Win32 handle-returning calls and owns the
            // handle until this drop.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn append_wide(target: &mut Vec<u16>, value: &OsStr) {
        target.extend(value.encode_wide());
    }

    fn append_ascii(target: &mut Vec<u16>, value: &str) {
        target.extend(value.encode_utf16());
    }

    fn show_warning(message: &str) {
        let message = wide(OsStr::new(message));
        let title = wide(OsStr::new(MIGRATION_TITLE));
        // SAFETY: both strings are valid NUL-terminated UTF-16 and a null owner is allowed for MessageBoxW.
        let _ = unsafe {
            MessageBoxW(null_mut(), message.as_ptr(), title.as_ptr(), MB_OK | MB_ICONWARNING)
        };
    }

    fn current_executable_path() -> Option<PathBuf> {
        let mut buffer = vec![0_u16; MAX_EXECUTABLE_PATH];
        // SAFETY: buffer is writable for the advertised element count; null module selects the current image.
        let length = unsafe {
            GetModuleFileNameW(null_mut(), buffer.as_mut_ptr(), u32::try_from(buffer.len()).ok()?)
        };
        let length = usize::try_from(length).ok()?;
        if length == 0 || length >= buffer.len() {
            return None;
        }
        Some(PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length])))
    }

    fn open_read_shared(path: &Path) -> Option<OwnedHandle> {
        let path = wide(path.as_os_str());
        // SAFETY: path is valid NUL-terminated UTF-16; null security/template pointers are documented defaults.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        OwnedHandle::from_raw(handle)
    }

    fn files_have_identical_bytes(lhs: &Path, rhs: &Path) -> bool {
        let Some(lhs) = open_read_shared(lhs) else { return false };
        let Some(rhs) = open_read_shared(rhs) else { return false };
        let mut lhs_size = 0_i64;
        let mut rhs_size = 0_i64;
        // SAFETY: both handles are valid readable files and output pointers are writable i64 values.
        if unsafe { GetFileSizeEx(lhs.0, &raw mut lhs_size) } == 0
            || unsafe { GetFileSizeEx(rhs.0, &raw mut rhs_size) } == 0
            || lhs_size != rhs_size
        {
            return false;
        }

        let mut lhs_buffer = [0_u8; 64 * 1024];
        let mut rhs_buffer = [0_u8; 64 * 1024];
        loop {
            let mut lhs_read = 0_u32;
            let mut rhs_read = 0_u32;
            // SAFETY: handles and buffers are valid and no OVERLAPPED structure is used for synchronous reads.
            let lhs_ok = unsafe {
                ReadFile(
                    lhs.0,
                    lhs_buffer.as_mut_ptr(),
                    lhs_buffer.len() as u32,
                    &raw mut lhs_read,
                    null_mut(),
                )
            };
            // SAFETY: same conditions as the lhs ReadFile call.
            let rhs_ok = unsafe {
                ReadFile(
                    rhs.0,
                    rhs_buffer.as_mut_ptr(),
                    rhs_buffer.len() as u32,
                    &raw mut rhs_read,
                    null_mut(),
                )
            };
            if lhs_ok == 0 || rhs_ok == 0 || lhs_read != rhs_read {
                return false;
            }
            let count = lhs_read as usize;
            if lhs_buffer[..count] != rhs_buffer[..count] {
                return false;
            }
            if lhs_read == 0 {
                return true;
            }
        }
    }

    fn write_migration_script(path: &Path) -> bool {
        let path_wide = wide(path.as_os_str());
        // SAFETY: path is NUL-terminated; CREATE_NEW prevents replacing an existing helper; null security/template
        // pointers are documented defaults.
        let handle = unsafe {
            CreateFileW(
                path_wide.as_ptr(),
                GENERIC_WRITE,
                0,
                null(),
                CREATE_NEW,
                FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_TEMPORARY,
                null_mut(),
            )
        };
        let Some(handle) = OwnedHandle::from_raw(handle) else { return false };
        let bytes = MIGRATION_SCRIPT.as_bytes();
        let Ok(byte_count) = u32::try_from(bytes.len()) else { return false };
        let mut written = 0_u32;
        // SAFETY: handle is writable, bytes is valid for byte_count bytes, and written is writable.
        let wrote = unsafe {
            WriteFile(handle.0, bytes.as_ptr(), byte_count, &raw mut written, null_mut())
        } != 0
            && written == byte_count;
        if wrote {
            // SAFETY: handle is a valid writable file; the result is intentionally advisory like C++.
            let _ = unsafe { FlushFileBuffers(handle.0) };
        }
        wrote
    }

    fn system_directory() -> Option<PathBuf> {
        let mut buffer = [0_u16; MAX_SYSTEM_PATH];
        // SAFETY: buffer is writable for MAX_SYSTEM_PATH UTF-16 code units.
        let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
        let length = usize::try_from(length).ok()?;
        if length == 0 || length >= buffer.len() {
            return None;
        }
        Some(PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length])))
    }

    fn delete_file_best_effort(path: &Path) {
        let path = wide(path.as_os_str());
        // SAFETY: path is NUL-terminated. Failure is intentionally ignored during rollback cleanup.
        let _ = unsafe { DeleteFileW(path.as_ptr()) };
    }

    fn helper_command_line(
        script_path: &Path,
        current_path: &Path,
        target_path: &Path,
        system_directory: &Path,
    ) -> Vec<u16> {
        let mut command = Vec::new();
        append_ascii(&mut command, "cmd.exe /D /Q /C \"\"");
        append_wide(&mut command, script_path.as_os_str());
        append_ascii(&mut command, "\" \"");
        append_wide(&mut command, current_path.as_os_str());
        append_ascii(&mut command, "\" \"");
        append_wide(&mut command, target_path.as_os_str());
        append_ascii(&mut command, "\" \"");
        append_wide(&mut command, system_directory.as_os_str());
        append_ascii(&mut command, "\"\"");
        command.push(0);
        command
    }

    #[must_use]
    pub fn schedule_v340_legacy_filename_migration() -> bool {
        let Some(current_path) = current_executable_path() else { return false };
        if !is_legacy_filename(&current_path) {
            return false;
        }

        let directory = current_path.parent().unwrap_or_else(|| Path::new("."));
        let target_path = migration_target_path(&current_path);
        let target_wide = wide(target_path.as_os_str());
        // SAFETY: target_wide is NUL-terminated.
        let target_attributes = unsafe { GetFileAttributesW(target_wide.as_ptr()) };
        let mut created_target = false;
        if target_attributes == INVALID_FILE_ATTRIBUTES {
            let current_wide = wide(current_path.as_os_str());
            // SAFETY: both paths are NUL-terminated; fail-if-exists prevents replacement on a race.
            if unsafe { CopyFileW(current_wide.as_ptr(), target_wide.as_ptr(), 1) } == 0 {
                show_warning(
                    "Spencer Macro Utilities V3.4.0 was installed through the legacy updater, but Windows prevented it from creating the new versioned executable filename. The app will continue using suspend.exe for this launch and will retry next time. You can also download the versioned executable from the GitHub releases page.",
                );
                return false;
            }
            created_target = true;
        } else if (target_attributes & FILE_ATTRIBUTE_DIRECTORY) != 0
            || !files_have_identical_bytes(&current_path, &target_path)
        {
            show_warning(
                "Spencer Macro Utilities V3.4.0 needs to rename the legacy suspend.exe installation, but a different file already exists at Spencer-Macro-Utilities-V3.4.0-Windows.exe. Nothing was overwritten. The app will continue using suspend.exe for this launch.",
            );
            return false;
        }

        let Some(system_directory) = system_directory() else {
            if created_target {
                delete_file_best_effort(&target_path);
            }
            show_warning(
                "Spencer Macro Utilities V3.4.0 could not resolve the Windows system directory needed for its filename migration. The app will continue using suspend.exe for this launch and will retry next time.",
            );
            return false;
        };

        // SAFETY: these two calls have no pointer preconditions and only read process/system counters.
        let process_id = unsafe { GetCurrentProcessId() };
        let tick_count = unsafe { GetTickCount64() };
        let script_path =
            directory.join(format!(".smu-v3.4.0-filename-migration-{process_id}-{tick_count}.cmd"));
        if !write_migration_script(&script_path) {
            delete_file_best_effort(&script_path);
            if created_target {
                delete_file_best_effort(&target_path);
            }
            show_warning(
                "Spencer Macro Utilities V3.4.0 could not create its one-time filename migration helper. The app will continue using suspend.exe for this launch and will retry next time.",
            );
            return false;
        }

        let command_interpreter = system_directory.join("cmd.exe");
        let command_interpreter_wide = wide(command_interpreter.as_os_str());
        let directory_wide = wide(directory.as_os_str());
        let mut command_line =
            helper_command_line(&script_path, &current_path, &target_path, &system_directory);
        let mut startup_info =
            STARTUPINFOW { cb: size_of::<STARTUPINFOW>() as u32, ..STARTUPINFOW::default() };
        let mut process_info = PROCESS_INFORMATION::default();
        // SAFETY: application/current-directory strings and mutable command line are NUL-terminated; startup and
        // process-info pointers are valid for this synchronous creation call.
        let created = unsafe {
            CreateProcessW(
                command_interpreter_wide.as_ptr(),
                command_line.as_mut_ptr(),
                null(),
                null(),
                0,
                CREATE_NO_WINDOW,
                null(),
                directory_wide.as_ptr(),
                &raw mut startup_info,
                &raw mut process_info,
            )
        };
        if created == 0 {
            delete_file_best_effort(&script_path);
            if created_target {
                delete_file_best_effort(&target_path);
            }
            show_warning(
                "Spencer Macro Utilities V3.4.0 could not start its one-time filename migration helper. The app will continue using suspend.exe for this launch and will retry next time.",
            );
            return false;
        }

        if let Some(thread) = OwnedHandle::from_raw(process_info.hThread) {
            drop(thread);
        }
        if let Some(process) = OwnedHandle::from_raw(process_info.hProcess) {
            drop(process);
        }
        true
    }
}

#[cfg(target_os = "windows")]
pub use platform::schedule_v340_legacy_filename_migration;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v340_tripwire_and_public_filename_match_release_contract() {
        assert_eq!(VERSION_BYTES, b"3.4.0\n");
        assert_eq!(V340_TARGET_FILENAME, "Spencer-Macro-Utilities-V3.4.0-Windows.exe");
        assert_eq!(MIGRATION_TITLE, "Spencer Macro Utilities filename migration");
    }

    #[test]
    fn only_suspend_exe_triggers_legacy_bridge_case_insensitively() {
        assert!(is_legacy_filename(Path::new("C:/SMU/suspend.exe")));
        assert!(is_legacy_filename(Path::new("C:/SMU/SUSPEND.EXE")));
        assert!(!is_legacy_filename(Path::new(
            r"C:\\SMU\\Spencer-Macro-Utilities-V3.4.0-Windows.exe"
        )));
    }

    #[test]
    fn target_stays_beside_legacy_executable() {
        assert_eq!(
            migration_target_path(Path::new("C:/SMU/suspend.exe")),
            PathBuf::from("C:/SMU/Spencer-Macro-Utilities-V3.4.0-Windows.exe")
        );
    }

    #[test]
    fn helper_uses_trusted_system_tools_five_minute_retry_and_self_cleanup() {
        assert!(MIGRATION_SCRIPT.contains("%SMU_SYSTEM%\\timeout.exe"));
        assert!(MIGRATION_SCRIPT.contains("SMU_DELETE_ATTEMPTS% GEQ 300"));
        assert!(MIGRATION_SCRIPT.contains("%SMU_SYSTEM%\\WindowsPowerShell\\v1.0\\powershell.exe"));
        assert!(MIGRATION_SCRIPT.contains("Remove-Item -LiteralPath $env:SMU_HELPER -Force"));
        assert!(MIGRATION_SCRIPT.contains("start \"\" \"%SMU_NEW%\""));
    }
}
