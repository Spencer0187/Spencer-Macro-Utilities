use crate::{
    ProcessControl, ProcessDiscovery, ProcessForeground, ProcessRecord,
    macos_process_candidate_matches, macos_select_main_pid,
};
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use std::io;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const FIND_ALL_CACHE_DURATION: Duration = Duration::from_millis(250);

#[derive(Clone, Debug)]
struct CachedProcesses {
    selector: String,
    pids: Vec<u32>,
    expires_at: Instant,
}

#[derive(Debug, Default)]
pub struct MacosProcessService {
    cache: Mutex<Option<CachedProcesses>>,
}

impl MacosProcessService {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn candidate_names(app: &NSRunningApplication) -> Vec<String> {
        let mut names = Vec::with_capacity(4);
        if let Some(value) = app.localizedName() {
            names.push(value.to_string());
        }
        if let Some(value) = app.bundleIdentifier() {
            names.push(value.to_string());
        }
        if let Some(value) = app.executableURL().and_then(|url| url.lastPathComponent()) {
            names.push(value.to_string());
        }
        if let Some(value) = app.bundleURL().and_then(|url| url.lastPathComponent()) {
            names.push(value.to_string());
        }
        names
    }

    fn app_matches(app: &NSRunningApplication, query: &str) -> bool {
        Self::candidate_names(app)
            .iter()
            .any(|candidate| macos_process_candidate_matches(candidate, query))
    }

    fn parse_pid(token: &str) -> Option<u32> {
        if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let pid = token.parse::<u32>().ok()?;
        (pid != 0).then_some(pid)
    }

    fn is_running_pid(pid: u32) -> bool {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        autoreleasepool(|_| {
            NSRunningApplication::runningApplicationWithProcessIdentifier(pid).is_some()
        })
    }

    fn uncached_find_all(selector: &str) -> Vec<u32> {
        let tokens: Vec<&str> = selector.split_ascii_whitespace().collect();
        if tokens.is_empty() {
            return Vec::new();
        }

        autoreleasepool(|_| {
            let workspace = NSWorkspace::sharedWorkspace();
            let running_apps = workspace.runningApplications();
            let mut pids = Vec::new();
            for token in tokens {
                if let Some(pid) = Self::parse_pid(token) {
                    if Self::is_running_pid(pid) {
                        pids.push(pid);
                    }
                    continue;
                }
                for index in 0..running_apps.count() {
                    let app = running_apps.objectAtIndex(index);
                    if !Self::app_matches(&app, token) {
                        continue;
                    }
                    let pid = app.processIdentifier();
                    if let Ok(pid) = u32::try_from(pid)
                        && pid != 0
                    {
                        pids.push(pid);
                    }
                }
            }
            pids.sort_unstable();
            pids.dedup();
            pids
        })
    }

    fn cached_find_all(&self, selector: &str) -> Vec<u32> {
        let now = Instant::now();
        if let Ok(cache) = self.cache.lock()
            && let Some(cached) = cache.as_ref()
            && cached.selector == selector
            && now < cached.expires_at
        {
            return cached.pids.clone();
        }

        let pids = Self::uncached_find_all(selector);
        if let Ok(mut cache) = self.cache.lock() {
            *cache = Some(CachedProcesses {
                selector: selector.to_owned(),
                pids: pids.clone(),
                expires_at: now + FIND_ALL_CACHE_DURATION,
            });
        }
        pids
    }

    fn frontmost_pid() -> Option<u32> {
        autoreleasepool(|_| {
            let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
            let pid = u32::try_from(app.processIdentifier()).ok()?;
            (pid != 0).then_some(pid)
        })
    }

    fn send_signal(pid: u32, signal: libc::c_int) -> bool {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        if pid == 0 || pid == unsafe { libc::getpid() } {
            return false;
        }
        // SAFETY: kill receives a validated nonzero process identifier and a fixed POSIX signal.
        unsafe { libc::kill(pid, signal) == 0 }
    }
}

impl ProcessDiscovery for MacosProcessService {
    fn snapshot(&self) -> io::Result<Vec<ProcessRecord>> {
        Ok(autoreleasepool(|_| {
            let running_apps = NSWorkspace::sharedWorkspace().runningApplications();
            let mut records = Vec::with_capacity(running_apps.count());
            for index in 0..running_apps.count() {
                let app = running_apps.objectAtIndex(index);
                let Ok(pid) = u32::try_from(app.processIdentifier()) else {
                    continue;
                };
                if pid == 0 {
                    continue;
                }
                records.push(ProcessRecord {
                    pid,
                    parent_pid: None,
                    command_name: app.localizedName().map(|value| value.to_string()),
                    executable_name: app
                        .executableURL()
                        .and_then(|url| url.lastPathComponent())
                        .map(|value| value.to_string()),
                });
            }
            records
        }))
    }

    fn find_process(&self, selector: &str) -> io::Result<Option<u32>> {
        self.find_main_process(selector)
    }

    fn find_all_processes(&self, selector: &str) -> io::Result<Vec<u32>> {
        Ok(self.cached_find_all(selector))
    }

    fn find_main_process(&self, selector: &str) -> io::Result<Option<u32>> {
        let pids = self.cached_find_all(selector);
        Ok(macos_select_main_pid(&pids, Self::frontmost_pid()))
    }
}

impl ProcessControl for MacosProcessService {
    fn suspend(&self, pid: u32) -> bool {
        Self::send_signal(pid, libc::SIGSTOP)
    }

    fn resume(&self, pid: u32) -> bool {
        Self::send_signal(pid, libc::SIGCONT)
    }
}

impl ProcessForeground for MacosProcessService {
    fn is_foreground_process(&self, pid: u32) -> bool {
        pid != 0 && Self::frontmost_pid() == Some(pid)
    }
}
