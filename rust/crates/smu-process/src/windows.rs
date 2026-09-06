use crate::{
    ProcessControl, ProcessDiscovery, ProcessForeground, ProcessRecord, newest_windows_process,
    windows_process_name_matches,
};
use std::{io, mem::size_of, sync::OnceLock};
use windows_sys::Win32::{
    Foundation::{CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        LibraryLoader::{GetModuleHandleA, GetProcAddress},
        Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SUSPEND_RESUME,
        },
    },
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
};

#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeProcessRow {
    pid: u32,
    parent_pid: Option<u32>,
    executable_name: String,
}

struct OwnedHandle(HANDLE);

impl OwnedHandle {
    fn from_snapshot(handle: HANDLE) -> Option<Self> {
        (!handle.is_null() && handle != INVALID_HANDLE_VALUE).then_some(Self(handle))
    }

    fn from_process(handle: HANDLE) -> Option<Self> {
        (!handle.is_null()).then_some(Self(handle))
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: the handle is owned by this wrapper and is closed exactly once here.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn executable_name(entry: &PROCESSENTRY32W) -> String {
    let length = entry
        .szExeFile
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(entry.szExeFile.len());
    String::from_utf16_lossy(&entry.szExeFile[..length])
}

fn process_rows() -> Vec<NativeProcessRow> {
    // SAFETY: Toolhelp takes plain values and returns an owned snapshot handle on success.
    let Some(snapshot) =
        OwnedHandle::from_snapshot(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) })
    else {
        return Vec::new();
    };

    let mut entry = PROCESSENTRY32W::default();
    entry.dwSize = u32::try_from(size_of::<PROCESSENTRY32W>()).unwrap_or_default();
    // SAFETY: `entry` points to writable storage with the required `dwSize` initialized.
    if unsafe { Process32FirstW(snapshot.0, &mut entry) } == 0 {
        return Vec::new();
    }

    let mut rows = Vec::new();
    loop {
        rows.push(NativeProcessRow {
            pid: entry.th32ProcessID,
            parent_pid: (entry.th32ParentProcessID != 0).then_some(entry.th32ParentProcessID),
            executable_name: executable_name(&entry),
        });
        // SAFETY: `entry` remains valid writable storage for the same Toolhelp snapshot.
        if unsafe { Process32NextW(snapshot.0, &mut entry) } == 0 {
            break;
        }
    }
    rows
}

fn open_process(pid: u32, access: u32) -> Option<OwnedHandle> {
    // SAFETY: `OpenProcess` does not retain caller memory; the returned handle is wrapped immediately.
    OwnedHandle::from_process(unsafe { OpenProcess(access, 0, pid) })
}

fn process_creation_time(pid: u32) -> Option<u64> {
    let process = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: all FILETIME pointers are valid writable values for the lifetime of the call.
    if unsafe { GetProcessTimes(process.0, &mut creation, &mut exit, &mut kernel, &mut user) } == 0
    {
        return None;
    }
    Some((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

type NtProcessFn = unsafe extern "system" fn(HANDLE) -> i32;

fn load_nt_process_fn(name: &'static [u8]) -> Option<NtProcessFn> {
    // SAFETY: both strings are static NUL-terminated byte strings and Windows owns the module handle.
    let module = unsafe { GetModuleHandleA(b"ntdll.dll\0".as_ptr()) };
    if module.is_null() {
        return None;
    }
    // SAFETY: `name` is a static NUL-terminated export name.
    let generic = unsafe { GetProcAddress(module, name.as_ptr()) }?;
    // SAFETY: NtSuspendProcess/NtResumeProcess have the documented one-HANDLE NTSTATUS signature.
    Some(unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, NtProcessFn>(generic)
    })
}

fn suspend_process_fn() -> Option<NtProcessFn> {
    static FUNCTION: OnceLock<Option<NtProcessFn>> = OnceLock::new();
    *FUNCTION.get_or_init(|| load_nt_process_fn(b"NtSuspendProcess\0"))
}

fn resume_process_fn() -> Option<NtProcessFn> {
    static FUNCTION: OnceLock<Option<NtProcessFn>> = OnceLock::new();
    *FUNCTION.get_or_init(|| load_nt_process_fn(b"NtResumeProcess\0"))
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsProcessBackend;

impl WindowsProcessBackend {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ProcessDiscovery for WindowsProcessBackend {
    fn snapshot(&self) -> io::Result<Vec<ProcessRecord>> {
        Ok(process_rows()
            .into_iter()
            .map(|row| ProcessRecord {
                pid: row.pid,
                parent_pid: row.parent_pid,
                command_name: Some(row.executable_name.clone()),
                executable_name: Some(row.executable_name),
            })
            .collect())
    }

    fn find_process(&self, selector: &str) -> io::Result<Option<u32>> {
        self.find_main_process(selector)
    }

    fn find_all_processes(&self, selector: &str) -> io::Result<Vec<u32>> {
        Ok(process_rows()
            .into_iter()
            .filter(|row| windows_process_name_matches(&row.executable_name, selector))
            .map(|row| row.pid)
            .collect())
    }

    fn find_main_process(&self, selector: &str) -> io::Result<Option<u32>> {
        let candidates = process_rows()
            .into_iter()
            .filter(|row| windows_process_name_matches(&row.executable_name, selector))
            .filter_map(|row| process_creation_time(row.pid).map(|creation| (row.pid, creation)))
            .collect::<Vec<_>>();
        Ok(newest_windows_process(&candidates))
    }
}

impl ProcessControl for WindowsProcessBackend {
    fn suspend(&self, pid: u32) -> bool {
        let Some(function) = suspend_process_fn() else {
            return false;
        };
        let Some(process) =
            open_process(pid, PROCESS_SUSPEND_RESUME | PROCESS_QUERY_LIMITED_INFORMATION)
        else {
            return false;
        };
        // SAFETY: `process` is a live process handle and the loaded function has the matching ABI.
        unsafe { function(process.0) >= 0 }
    }

    fn resume(&self, pid: u32) -> bool {
        let Some(function) = resume_process_fn() else {
            return false;
        };
        let Some(process) =
            open_process(pid, PROCESS_SUSPEND_RESUME | PROCESS_QUERY_LIMITED_INFORMATION)
        else {
            return false;
        };
        // SAFETY: `process` is a live process handle and the loaded function has the matching ABI.
        unsafe { function(process.0) >= 0 }
    }
}

impl ProcessForeground for WindowsProcessBackend {
    fn is_foreground_process(&self, pid: u32) -> bool {
        // SAFETY: `GetForegroundWindow` has no caller-owned pointer arguments.
        let window = unsafe { GetForegroundWindow() };
        if window.is_null() {
            return false;
        }
        let mut foreground_pid = 0_u32;
        // SAFETY: `foreground_pid` is valid writable storage for the duration of the call.
        unsafe {
            GetWindowThreadProcessId(window, &mut foreground_pid);
        }
        foreground_pid == pid
    }
}
