use super::ProcDiscovery;
use crate::{ProcessDiscovery, ProcessForeground, ProcessRecord};
use std::collections::BTreeSet;
use std::env;
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
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

#[derive(Debug, Default)]
pub struct X11rbForegroundSource;

impl ForegroundPidSource for X11rbForegroundSource {
    fn foreground_pid(&self) -> Option<u32> {
        let (connection, screen_index) = x11rb::connect(None).ok()?;
        let root = connection.setup().roots.get(screen_index)?.root;
        let active_atom =
            connection.intern_atom(true, b"_NET_ACTIVE_WINDOW").ok()?.reply().ok()?.atom;
        let pid_atom = connection.intern_atom(true, b"_NET_WM_PID").ok()?.reply().ok()?.atom;
        if active_atom == x11rb::NONE || pid_atom == x11rb::NONE {
            return None;
        }

        let active_reply = connection
            .get_property(false, root, active_atom, AtomEnum::WINDOW, 0, 1)
            .ok()?
            .reply()
            .ok()?;
        let active_window = active_reply.value32()?.next()?;
        if active_window == x11rb::NONE {
            return None;
        }

        let pid_reply = connection
            .get_property(false, active_window, pid_atom, AtomEnum::CARDINAL, 0, 1)
            .ok()?
            .reply()
            .ok()?;
        let pid = pid_reply.value32()?.next()?;
        (pid != 0).then_some(pid)
    }
}

pub struct ProcForegroundProcess {
    proc_root: PathBuf,
    cgroup_root: PathBuf,
    display_server: Arc<dyn DisplayServerSource>,
    foreground: Arc<dyn ForegroundPidSource>,
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
            display_server: Arc::new(EnvironmentDisplayServer),
            foreground: Arc::new(X11rbForegroundSource),
        }
    }
}

impl ProcForegroundProcess {
    #[must_use]
    pub fn new(proc_root: impl Into<PathBuf>, cgroup_root: impl Into<PathBuf>) -> Self {
        Self { proc_root: proc_root.into(), cgroup_root: cgroup_root.into(), ..Self::default() }
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
        let target_cgroups: BTreeSet<PathBuf> =
            target_pids.iter().filter_map(|pid| self.cgroup_path(*pid)).collect();
        if target_cgroups.is_empty() {
            return None;
        }

        for record in records {
            if record.pid == namespace_pid {
                continue;
            }
            let Some(cgroup) = self.cgroup_path(record.pid) else {
                continue;
            };
            if !target_cgroups.contains(&cgroup) {
                continue;
            }
            if self.namespace_pids(record.pid).contains(&namespace_pid) {
                return Some(record.pid);
            }
        }
        None
    }
}

impl ProcessForeground for ProcForegroundProcess {
    fn is_foreground_process(&self, pid: u32) -> bool {
        if self.display_server.display_server() != DisplayServer::X11 {
            return false;
        }
        let records = ProcDiscovery::new(&self.proc_root).snapshot().unwrap_or_default();
        let candidates = process_tree_from_records(pid, &records);
        if candidates.is_empty() {
            return false;
        }
        let Some(foreground_pid) = self.foreground.foreground_pid() else {
            return false;
        };
        if candidates.contains(&foreground_pid) {
            return true;
        }
        self.host_pid_for_namespace_pid(foreground_pid, &candidates, &records)
            .is_some_and(|host_pid| candidates.contains(&host_pid))
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
