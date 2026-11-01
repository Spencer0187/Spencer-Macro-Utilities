use super::ProcDiscovery;
use crate::{ProcessForeground, ProcessRecord};
use std::collections::{BTreeSet, HashMap};
use std::env;
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

// The C++ controller shares one foreground refresh across all macros every 25 ms.
const FOREGROUND_CACHE_INTERVAL: Duration = Duration::from_millis(25);
const NAMESPACE_CACHE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Default)]
struct NamespaceCache {
    checked_at: Option<Instant>,
    results: HashMap<(u32, Vec<u32>), Option<u32>>,
    cgroups: HashMap<u32, Option<PathBuf>>,
    namespace_pids: HashMap<u32, Vec<u32>>,
}

#[derive(Debug, Default)]
struct ForegroundCache {
    checked_at: Option<Instant>,
    // Distinguish an unqueried interval from an interval with no active X11 window.
    active_pid_queried: bool,
    active_pid: Option<u32>,
    results: HashMap<u32, bool>,
}
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayServer {
    X11,
    Wayland,
    Unknown,
}

#[must_use]
fn detect_display_server_values(
    session_type: Option<&str>,
    wayland_display: bool,
    x11_display: bool,
) -> DisplayServer {
    let session_type = session_type.unwrap_or_default().to_ascii_lowercase();
    if session_type == "wayland" || wayland_display {
        DisplayServer::Wayland
    } else if session_type == "x11" || x11_display {
        DisplayServer::X11
    } else {
        DisplayServer::Unknown
    }
}

#[must_use]
pub fn detect_display_server() -> DisplayServer {
    detect_display_server_values(
        env::var("XDG_SESSION_TYPE").ok().as_deref(),
        env::var_os("WAYLAND_DISPLAY").is_some(),
        env::var_os("DISPLAY").is_some(),
    )
}

#[must_use]
pub fn foreground_detection_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(probe_foreground_detection_available)
}

fn probe_foreground_detection_available() -> bool {
    if detect_display_server() != DisplayServer::X11 {
        return false;
    }

    let Ok((connection, screen_index)) = x11rb::connect(None) else {
        return false;
    };
    let Some(screen) = connection.setup().roots.get(screen_index) else {
        return false;
    };
    let Ok(active_cookie) = connection.intern_atom(true, b"_NET_ACTIVE_WINDOW") else {
        return false;
    };
    let Ok(active_reply) = active_cookie.reply() else {
        return false;
    };
    let Ok(pid_cookie) = connection.intern_atom(true, b"_NET_WM_PID") else {
        return false;
    };
    let Ok(pid_reply) = pid_cookie.reply() else {
        return false;
    };
    if active_reply.atom == x11rb::NONE || pid_reply.atom == x11rb::NONE {
        return false;
    }

    let Ok(property_cookie) =
        connection.get_property(false, screen.root, active_reply.atom, AtomEnum::WINDOW, 0, 1)
    else {
        return false;
    };
    let Ok(property) = property_cookie.reply() else {
        return false;
    };
    property.type_ == u32::from(AtomEnum::WINDOW)
        && property.format == 32
        && property.value_len >= 1
}

pub trait DisplayServerSource: Send + Sync {
    fn display_server(&self) -> DisplayServer;
}

#[derive(Debug, Default)]
pub struct EnvironmentDisplayServer;

impl DisplayServerSource for EnvironmentDisplayServer {
    fn display_server(&self) -> DisplayServer {
        detect_display_server()
    }
}

pub trait ForegroundPidSource: Send + Sync {
    fn foreground_pid(&self) -> Option<u32>;
}

struct X11ForegroundSession {
    connection: x11rb::rust_connection::RustConnection,
    root: u32,
    active_atom: u32,
    pid_atom: u32,
}

impl X11ForegroundSession {
    fn connect() -> Option<Self> {
        let (connection, screen_index) = x11rb::connect(None).ok()?;
        let root = connection.setup().roots.get(screen_index)?.root;
        let active_atom =
            connection.intern_atom(true, b"_NET_ACTIVE_WINDOW").ok()?.reply().ok()?.atom;
        let pid_atom = connection.intern_atom(true, b"_NET_WM_PID").ok()?.reply().ok()?.atom;
        if active_atom == x11rb::NONE || pid_atom == x11rb::NONE {
            return None;
        }
        Some(Self { connection, root, active_atom, pid_atom })
    }

    fn foreground_pid(&self) -> Result<Option<u32>, ()> {
        let active_reply = self
            .connection
            .get_property(false, self.root, self.active_atom, AtomEnum::WINDOW, 0, 1)
            .map_err(|_| ())?
            .reply()
            .map_err(|_| ())?;
        let active_window =
            active_reply.value32().and_then(|mut values| values.next()).unwrap_or(0);
        if active_window == x11rb::NONE {
            return Ok(None);
        }
        let pid_reply = self
            .connection
            .get_property(false, active_window, self.pid_atom, AtomEnum::CARDINAL, 0, 1)
            .map_err(|_| ())?
            .reply()
            .map_err(|_| ())?;
        let pid = pid_reply.value32().and_then(|mut values| values.next()).unwrap_or(0);
        Ok((pid != 0).then_some(pid))
    }
}

#[derive(Default)]
pub struct X11rbForegroundSource {
    session: Mutex<Option<X11ForegroundSession>>,
}

impl fmt::Debug for X11rbForegroundSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("X11rbForegroundSource").finish_non_exhaustive()
    }
}

impl ForegroundPidSource for X11rbForegroundSource {
    fn foreground_pid(&self) -> Option<u32> {
        let mut session = self.session.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if session.is_none() {
            *session = X11ForegroundSession::connect();
        }
        let result = session.as_ref()?.foreground_pid();
        let pid = result.unwrap_or_else(|()| {
            // Reconnect after X server restart or an underlying transport failure.
            *session = None;
            None
        });
        drop(session);
        pid
    }
}

pub struct ProcForegroundProcess {
    proc_root: PathBuf,
    cgroup_root: PathBuf,
    discovery: Arc<ProcDiscovery>,
    display_server: Arc<dyn DisplayServerSource>,
    foreground: Arc<dyn ForegroundPidSource>,
    cache: Mutex<ForegroundCache>,
    namespace_cache: Mutex<NamespaceCache>,
}

impl fmt::Debug for ProcForegroundProcess {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProcForegroundProcess")
            .field("proc_root", &self.proc_root)
            .field("cgroup_root", &self.cgroup_root)
            .finish_non_exhaustive()
    }
}

impl Default for ProcForegroundProcess {
    fn default() -> Self {
        Self {
            proc_root: PathBuf::from("/proc"),
            cgroup_root: PathBuf::from("/sys/fs/cgroup"),
            discovery: Arc::new(ProcDiscovery::default()),
            display_server: Arc::new(EnvironmentDisplayServer),
            foreground: Arc::new(X11rbForegroundSource::default()),
            cache: Mutex::new(ForegroundCache::default()),
            namespace_cache: Mutex::new(NamespaceCache::default()),
        }
    }
}

impl ProcForegroundProcess {
    #[must_use]
    pub fn new(proc_root: impl Into<PathBuf>, cgroup_root: impl Into<PathBuf>) -> Self {
        let proc_root = proc_root.into();
        Self {
            discovery: Arc::new(ProcDiscovery::new(&proc_root)),
            proc_root,
            cgroup_root: cgroup_root.into(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn with_discovery(mut self, discovery: Arc<ProcDiscovery>) -> Self {
        self.proc_root.clone_from(&discovery.proc_root);
        self.discovery = discovery;
        self
    }

    #[must_use]
    pub fn with_sources(
        mut self,
        display_server: Arc<dyn DisplayServerSource>,
        foreground: Arc<dyn ForegroundPidSource>,
    ) -> Self {
        self.display_server = display_server;
        self.foreground = foreground;
        self
    }

    fn cgroup_path(&self, pid: u32) -> Option<PathBuf> {
        let contents =
            fs::read_to_string(self.proc_root.join(pid.to_string()).join("cgroup")).ok()?;
        let relative = contents.lines().find_map(|line| line.strip_prefix("0::/"))?;
        Some(self.cgroup_root.join(relative))
    }

    fn namespace_pids(&self, pid: u32) -> Vec<u32> {
        let Ok(contents) = fs::read_to_string(self.proc_root.join(pid.to_string()).join("status"))
        else {
            return Vec::new();
        };
        contents.lines().find_map(|line| line.strip_prefix("NSpid:")).map_or_else(
            Vec::new,
            |values| {
                values
                    .split_ascii_whitespace()
                    .filter_map(|value| value.parse::<u32>().ok())
                    .collect()
            },
        )
    }

    fn host_pid_for_namespace_pid(
        &self,
        namespace_pid: u32,
        target_pids: &[u32],
        records: &[ProcessRecord],
    ) -> Option<u32> {
        if namespace_pid == 0 {
            return None;
        }
        let now = Instant::now();
        let mut cache =
            self.namespace_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if cache
            .checked_at
            .is_none_or(|checked| now.duration_since(checked) >= NAMESPACE_CACHE_INTERVAL)
        {
            cache.results.clear();
            cache.cgroups.clear();
            cache.namespace_pids.clear();
            cache.checked_at = Some(now);
        }
        let key = (namespace_pid, target_pids.to_vec());
        if let Some(&result) = cache.results.get(&key) {
            return result;
        }
        // A missing match is cached as well. Most native X11 foreground PIDs do not belong
        // to the target's PID namespace; without negative caching each 25 ms foreground poll
        // rereads every /proc/<pid>/cgroup and /proc/<pid>/status in the system.
        let result = self.resolve_namespace_pid(namespace_pid, target_pids, records, &mut cache);
        cache.results.insert(key, result);
        result
    }

    fn resolve_namespace_pid(
        &self,
        namespace_pid: u32,
        target_pids: &[u32],
        records: &[ProcessRecord],
        cache: &mut NamespaceCache,
    ) -> Option<u32> {
        let target_cgroups: BTreeSet<PathBuf> = target_pids
            .iter()
            .filter_map(|pid| {
                cache.cgroups.entry(*pid).or_insert_with(|| self.cgroup_path(*pid)).clone()
            })
            .collect();
        if target_cgroups.is_empty() {
            return None;
        }

        for record in records {
            if record.pid == namespace_pid {
                continue;
            }
            let Some(cgroup) =
                cache.cgroups.entry(record.pid).or_insert_with(|| self.cgroup_path(record.pid))
            else {
                continue;
            };
            if !target_cgroups.contains(cgroup) {
                continue;
            }
            if cache
                .namespace_pids
                .entry(record.pid)
                .or_insert_with(|| self.namespace_pids(record.pid))
                .contains(&namespace_pid)
            {
                return Some(record.pid);
            }
        }
        None
    }
}

impl ProcForegroundProcess {
    fn evaluate_foreground(&self, pid: u32, foreground_pid: Option<u32>) -> bool {
        if self.display_server.display_server() != DisplayServer::X11 {
            return false;
        }
        let Some(foreground_pid) = foreground_pid else {
            return false;
        };
        let Ok(records) = self.discovery.snapshot_shared() else {
            return false;
        };
        let candidates = process_tree_from_records(pid, &records);
        if candidates.is_empty() {
            return false;
        }
        if candidates.contains(&foreground_pid) {
            return true;
        }
        self.host_pid_for_namespace_pid(foreground_pid, &candidates, &records)
            .is_some_and(|host_pid| candidates.contains(&host_pid))
    }
}

impl ProcessForeground for ProcForegroundProcess {
    fn is_foreground_process(&self, pid: u32) -> bool {
        let now = Instant::now();
        let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if cache
            .checked_at
            .is_none_or(|checked| now.duration_since(checked) >= FOREGROUND_CACHE_INTERVAL)
        {
            cache.results.clear();
            cache.active_pid = None;
            cache.active_pid_queried = false;
            cache.checked_at = Some(now);
        }
        if let Some(&result) = cache.results.get(&pid) {
            return result;
        }
        if !cache.active_pid_queried {
            cache.active_pid = self.foreground.foreground_pid();
            cache.active_pid_queried = true;
        }
        let result = self.evaluate_foreground(pid, cache.active_pid);
        cache.results.insert(pid, result);
        result
    }
}

fn process_tree_from_records(root_pid: u32, records: &[ProcessRecord]) -> Vec<u32> {
    if !records.iter().any(|record| record.pid == root_pid) {
        return Vec::new();
    }
    let mut result = vec![root_pid];
    let mut frontier = vec![root_pid];
    while let Some(parent) = frontier.pop() {
        for record in records {
            if record.parent_pid == Some(parent) && !result.contains(&record.pid) {
                result.push(record.pid);
                frontier.push(record.pid);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Debug)]
    struct FixedServer(DisplayServer);

    impl DisplayServerSource for FixedServer {
        fn display_server(&self) -> DisplayServer {
            self.0
        }
    }

    #[derive(Debug)]
    struct FixedForeground(Option<u32>);

    impl ForegroundPidSource for FixedForeground {
        fn foreground_pid(&self) -> Option<u32> {
            self.0
        }
    }

    #[derive(Debug)]
    struct CountingForeground {
        pid: u32,
        calls: AtomicUsize,
    }

    impl ForegroundPidSource for CountingForeground {
        fn foreground_pid(&self) -> Option<u32> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Some(self.pid)
        }
    }

    #[test]
    fn shared_foreground_cache_reuses_result_within_25_ms() {
        let (root, proc_root, cgroup_root) = temp_roots("cached");
        write_proc(&proc_root, 20, 1, "app-test.scope", &[20]);
        let source = Arc::new(CountingForeground { pid: 20, calls: AtomicUsize::new(0) });
        let foreground = ProcForegroundProcess::new(&proc_root, &cgroup_root)
            .with_sources(Arc::new(FixedServer(DisplayServer::X11)), source.clone());
        assert!(foreground.is_foreground_process(20));
        assert!(foreground.is_foreground_process(20));
        assert_eq!(source.calls.load(Ordering::Relaxed), 1);
        fs::remove_dir_all(root).expect("fixture should be removable");
    }

    fn temp_roots(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("smu-process-foreground-{name}-{nonce}"));
        let proc_root = root.join("proc");
        let cgroup_root = root.join("cgroup");
        fs::create_dir_all(&proc_root).expect("fake proc root should be created");
        fs::create_dir_all(&cgroup_root).expect("fake cgroup root should be created");
        (root, proc_root, cgroup_root)
    }

    fn write_proc(proc_root: &Path, pid: u32, parent: u32, cgroup: &str, namespace_pids: &[u32]) {
        let process = proc_root.join(pid.to_string());
        fs::create_dir_all(&process).expect("fake process should be created");
        fs::write(process.join("comm"), format!("p{pid}\n")).expect("comm should be written");
        fs::write(process.join("stat"), format!("{pid} (p{pid}) S {parent} 0 0 0\n"))
            .expect("stat should be written");
        let namespaces = namespace_pids.iter().map(u32::to_string).collect::<Vec<_>>().join(" ");
        fs::write(process.join("status"), format!("NSpid:\t{namespaces}\n"))
            .expect("status should be written");
        fs::write(process.join("cgroup"), format!("0::/{cgroup}\n"))
            .expect("cgroup should be written");
    }

    fn foreground(
        proc_root: &Path,
        cgroup_root: &Path,
        server: DisplayServer,
        pid: Option<u32>,
    ) -> ProcForegroundProcess {
        ProcForegroundProcess::new(proc_root, cgroup_root)
            .with_sources(Arc::new(FixedServer(server)), Arc::new(FixedForeground(pid)))
    }

    #[test]
    fn display_server_detection_preserves_wayland_precedence() {
        assert_eq!(detect_display_server_values(Some("X11"), true, true), DisplayServer::Wayland);
        assert_eq!(detect_display_server_values(Some("x11"), false, false), DisplayServer::X11);
        assert_eq!(detect_display_server_values(None, false, true), DisplayServer::X11);
        assert_eq!(detect_display_server_values(None, false, false), DisplayServer::Unknown);
    }

    #[test]
    fn wayland_and_unknown_always_report_not_foreground() {
        let (root, proc_root, cgroup_root) = temp_roots("non-x11");
        write_proc(&proc_root, 10, 1, "app-test.scope", &[10]);
        assert!(
            !foreground(&proc_root, &cgroup_root, DisplayServer::Wayland, Some(10))
                .is_foreground_process(10)
        );
        assert!(
            !foreground(&proc_root, &cgroup_root, DisplayServer::Unknown, Some(10))
                .is_foreground_process(10)
        );
        fs::remove_dir_all(root).expect("fixture should be removable");
    }

    #[test]
    fn x11_foreground_accepts_root_or_descendant_pid() {
        let (root, proc_root, cgroup_root) = temp_roots("tree");
        write_proc(&proc_root, 20, 1, "app-test.scope", &[20]);
        write_proc(&proc_root, 21, 20, "app-test.scope", &[21]);
        assert!(
            foreground(&proc_root, &cgroup_root, DisplayServer::X11, Some(20))
                .is_foreground_process(20)
        );
        assert!(
            foreground(&proc_root, &cgroup_root, DisplayServer::X11, Some(21))
                .is_foreground_process(20)
        );
        assert!(
            !foreground(&proc_root, &cgroup_root, DisplayServer::X11, Some(99))
                .is_foreground_process(20)
        );
        fs::remove_dir_all(root).expect("fixture should be removable");
    }

    #[test]
    fn namespace_pid_is_mapped_to_host_pid_in_target_cgroup() {
        let (root, proc_root, cgroup_root) = temp_roots("namespace");
        write_proc(&proc_root, 1000, 1, "user.slice/app-sober.scope", &[1000]);
        write_proc(&proc_root, 1001, 1000, "user.slice/app-sober.scope", &[1001, 55]);
        assert!(
            foreground(&proc_root, &cgroup_root, DisplayServer::X11, Some(55))
                .is_foreground_process(1000)
        );
        fs::remove_dir_all(root).expect("fixture should be removable");
    }

    #[test]
    fn namespace_pid_in_other_cgroup_is_not_accepted() {
        let (root, proc_root, cgroup_root) = temp_roots("namespace-other");
        write_proc(&proc_root, 1000, 1, "user.slice/app-sober.scope", &[1000]);
        write_proc(&proc_root, 2000, 1, "other.scope", &[2000, 55]);
        assert!(
            !foreground(&proc_root, &cgroup_root, DisplayServer::X11, Some(55))
                .is_foreground_process(1000)
        );
        fs::remove_dir_all(root).expect("fixture should be removable");
    }

    #[test]
    fn missing_root_or_foreground_pid_returns_false() {
        let (root, proc_root, cgroup_root) = temp_roots("missing");
        assert!(
            !foreground(&proc_root, &cgroup_root, DisplayServer::X11, None)
                .is_foreground_process(10)
        );
        assert!(
            !foreground(&proc_root, &cgroup_root, DisplayServer::X11, Some(10))
                .is_foreground_process(10)
        );
        fs::remove_dir_all(root).expect("fixture should be removable");
    }
}
