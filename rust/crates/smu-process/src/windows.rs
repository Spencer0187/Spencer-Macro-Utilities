use crate::{
    ProcessControl, ProcessDiscovery, ProcessForeground, ProcessRecord, newest_windows_process,
    windows_process_name_matches,
};
use std::{
    io,
    mem::size_of,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, FILETIME, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, RECT},
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
    UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GetForegroundWindow, GetWindow, GetWindowRect,
        GetWindowThreadProcessId, HWND_NOTOPMOST, HWND_TOPMOST, IsWindowVisible, SW_RESTORE,
        SWP_NOMOVE, SWP_NOSIZE, SetForegroundWindow, SetWindowPos, ShowWindow,
    },
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

    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).unwrap_or_default(),
        ..PROCESSENTRY32W::default()
    };
    // SAFETY: `entry` points to writable storage with the required `dwSize` initialized.
    if unsafe { Process32FirstW(snapshot.0, &raw mut entry) } == 0 {
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
        if unsafe { Process32NextW(snapshot.0, &raw mut entry) } == 0 {
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
    if unsafe {
        GetProcessTimes(process.0, &raw mut creation, &raw mut exit, &raw mut kernel, &raw mut user)
    } == 0
    {
        return None;
    }
    Some((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowsWindowRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowsWindowHandle(HWND);

impl WindowsWindowHandle {
    const fn raw(self) -> HWND {
        self.0
    }

    #[must_use]
    pub fn rect(self) -> Option<WindowsWindowRect> {
        let mut rect = RECT::default();
        // SAFETY: `rect` is valid writable storage and the HWND is only passed back to Win32.
        (unsafe { GetWindowRect(self.raw(), &raw mut rect) } != 0).then_some(WindowsWindowRect {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        })
    }

    pub fn show_restore(self) {
        // SAFETY: ShowWindow retains no Rust references; the handle came from Win32 enumeration.
        unsafe {
            let _ = ShowWindow(self.raw(), SW_RESTORE);
        }
    }

    pub fn set_topmost(self, active: bool) {
        let insert_after = if active { HWND_TOPMOST } else { HWND_NOTOPMOST };
        // SAFETY: this changes only z-order; no caller memory is retained.
        unsafe {
            let _ = SetWindowPos(self.raw(), insert_after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
        }
    }

    pub fn set_foreground(self) {
        // SAFETY: SetForegroundWindow retains no Rust references.
        unsafe {
            let _ = SetForegroundWindow(self.raw());
        }
    }
}

#[derive(Debug)]
struct ProcessWindowSearchContext {
    pids: *const u32,
    pid_count: usize,
    window: HWND,
    creation_time: u64,
    found_any: bool,
}

unsafe extern "system" fn find_newest_process_window_callback(hwnd: HWND, lparam: LPARAM) -> i32 {
    if unsafe { IsWindowVisible(hwnd) } == 0 || !unsafe { GetWindow(hwnd, GW_OWNER) }.is_null() {
        return 1;
    }
    // SAFETY: `lparam` points at the stack context owned by `find_newest_process_window` for the
    // synchronous duration of EnumWindows.
    let context = unsafe { &mut *(lparam as *mut ProcessWindowSearchContext) };
    // SAFETY: the pointer/length pair originates from the live `pids` slice and remains valid until
    // EnumWindows returns.
    let pids = unsafe { std::slice::from_raw_parts(context.pids, context.pid_count) };
    let mut window_pid = 0_u32;
    // SAFETY: `window_pid` is valid writable storage for the duration of the call.
    unsafe {
        GetWindowThreadProcessId(hwnd, &raw mut window_pid);
    }
    if !pids.contains(&window_pid) {
        return 1;
    }
    let Some(creation_time) = process_creation_time(window_pid) else {
        return 1;
    };
    if !context.found_any || creation_time > context.creation_time {
        context.window = hwnd;
        context.creation_time = creation_time;
        context.found_any = true;
    }
    1
}

#[must_use]
pub fn find_newest_process_window(pids: &[u32]) -> Option<WindowsWindowHandle> {
    if pids.is_empty() {
        return None;
    }
    let mut context = ProcessWindowSearchContext {
        pids: pids.as_ptr(),
        pid_count: pids.len(),
        window: std::ptr::null_mut(),
        creation_time: 0,
        found_any: false,
    };
    // SAFETY: EnumWindows invokes the callback synchronously while `context` and `pids` remain live.
    unsafe {
        let _ =
            EnumWindows(Some(find_newest_process_window_callback), (&raw mut context) as LPARAM);
    }
    context.found_any.then_some(WindowsWindowHandle(context.window))
}

#[must_use]
pub fn foreground_window() -> Option<WindowsWindowHandle> {
    // SAFETY: GetForegroundWindow has no pointer arguments and retains no memory.
    let window = unsafe { GetForegroundWindow() };
    (!window.is_null()).then_some(WindowsWindowHandle(window))
}

type NtProcessFn = unsafe extern "system" fn(HANDLE) -> i32;

fn load_nt_process_fn(name: &'static [u8]) -> Option<NtProcessFn> {
    // SAFETY: both strings are static NUL-terminated byte strings and Windows owns the module handle.
    let module = unsafe { GetModuleHandleA(c"ntdll.dll".as_ptr().cast()) };
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

// All Windows controllers share the same ToolHelp process table. Each still applies its own
// selector, but a hundred independent scans per second are never necessary at idle.
const WINDOWS_PROCESS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Default)]
struct WindowsProcessCache {
    refreshed_at: Option<Instant>,
    rows: Arc<Vec<NativeProcessRow>>,
}

#[derive(Clone, Debug, Default)]
pub struct WindowsProcessBackend {
    cache: Arc<Mutex<WindowsProcessCache>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl WindowsProcessBackend {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn cached_rows(&self) -> Arc<Vec<NativeProcessRow>> {
        let mut cache = lock(&self.cache);
        let now = Instant::now();
        if cache
            .refreshed_at
            .is_some_and(|checked| now.duration_since(checked) < WINDOWS_PROCESS_REFRESH_INTERVAL)
        {
            return Arc::clone(&cache.rows);
        }
        cache.rows = Arc::new(process_rows());
        cache.refreshed_at = Some(Instant::now());
        Arc::clone(&cache.rows)
    }
}

impl ProcessDiscovery for WindowsProcessBackend {
    fn snapshot(&self) -> io::Result<Vec<ProcessRecord>> {
        Ok(self
            .cached_rows()
            .iter()
            .map(|row| ProcessRecord {
                pid: row.pid,
                parent_pid: row.parent_pid,
                command_name: Some(row.executable_name.clone()),
                executable_name: Some(row.executable_name.clone()),
            })
            .collect())
    }

    fn find_process(&self, selector: &str) -> io::Result<Option<u32>> {
        self.find_main_process(selector)
    }

    fn find_all_processes(&self, selector: &str) -> io::Result<Vec<u32>> {
        Ok(self
            .cached_rows()
            .iter()
            .filter(|row| windows_process_name_matches(&row.executable_name, selector))
            .map(|row| row.pid)
            .collect())
    }

    fn find_main_process(&self, selector: &str) -> io::Result<Option<u32>> {
        let candidates = self
            .cached_rows()
            .iter()
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
            GetWindowThreadProcessId(window, &raw mut foreground_pid);
        }
        foreground_pid == pid
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    #[test]
    fn cloned_windows_backend_reuses_process_table_until_refresh_due() {
        let backend = WindowsProcessBackend::new();
        let clone = backend.clone();
        let first = backend.cached_rows();
        let reused = clone.cached_rows();
        assert!(Arc::ptr_eq(&first, &reused));
        assert!(!first.is_empty());

        let mut cache = lock(&backend.cache);
        cache.refreshed_at =
            Instant::now().checked_sub(WINDOWS_PROCESS_REFRESH_INTERVAL + Duration::from_secs(1));
        drop(cache);
        let refreshed = clone.cached_rows();
        assert!(!Arc::ptr_eq(&first, &refreshed));
    }
}
