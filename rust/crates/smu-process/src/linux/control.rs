use super::ProcDiscovery;
use crate::{ProcessControl, ProcessDiscovery, ProcessRecord};
use std::fmt;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const FREEZE_GROUP_NAME: &str = "smu_freeze";
const HELPER_TIMEOUT: Duration = Duration::from_secs(2);
const HELPER_RESPONSE_LIMIT: u64 = 1024;

trait SignalSender: Send + Sync {
    fn send(&self, pid: u32, signal: i32) -> Result<(), i32>;
}

#[derive(Debug, Default)]
struct LibcSignalSender;

impl SignalSender for LibcSignalSender {
    fn send(&self, pid: u32, signal: i32) -> Result<(), i32> {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return Err(libc::EINVAL);
        };
        // SAFETY: `kill` does not retain pointers; `pid` and `signal` are plain integer values.
        let result = unsafe { libc::kill(pid, signal) };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error().raw_os_error().unwrap_or(libc::EIO))
        }
    }
}

/// Client used when ptrace makes SIGSTOP unreliable and cgroup writes require elevation.
pub trait FreezeHelperClient: Send + Sync {
    fn is_available(&self) -> bool;
    fn set_suspended(&self, pid: u32, suspended: bool) -> bool;
}

/// UI/application-owned authorization gate for starting the temporary privileged helper.
pub trait FreezeHelperAuthorization: Send + Sync {
    fn wait_until_authorized(&self) -> bool;
}

#[derive(Debug, Default)]
struct DenyFreezeHelperAuthorization;

impl FreezeHelperAuthorization for DenyFreezeHelperAuthorization {
    fn wait_until_authorized(&self) -> bool {
        false
    }
}

/// Client for the existing root `processhelper` Unix-socket protocol.
#[derive(Clone, Debug)]
pub struct UnixFreezeHelper {
    socket_path: PathBuf,
}

impl Default for UnixFreezeHelper {
    fn default() -> Self {
        // SAFETY: `getuid` has no preconditions and does not access caller-owned memory.
        let uid = unsafe { libc::getuid() };
        Self::new(format!("/tmp/smu-processhelper-{uid}.sock"))
    }
}

impl UnixFreezeHelper {
    #[must_use]
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self { socket_path: socket_path.into() }
    }

    fn root_peer(stream: &UnixStream) -> bool {
        let mut credentials = libc::ucred { pid: 0, uid: 0, gid: 0 };
        let Ok(mut length) = libc::socklen_t::try_from(std::mem::size_of::<libc::ucred>()) else {
            return false;
        };
        // SAFETY: `credentials` and `length` are valid writable objects for the duration of the
        // call; the socket fd is borrowed from a live `UnixStream`.
        let result = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                std::ptr::addr_of_mut!(credentials).cast(),
                std::ptr::addr_of_mut!(length),
            )
        };
        result == 0
            && usize::try_from(length).ok() == Some(std::mem::size_of::<libc::ucred>())
            && credentials.uid == 0
    }

    fn command(&self, command: &str) -> Option<String> {
        let mut stream = UnixStream::connect(&self.socket_path).ok()?;
        stream.set_read_timeout(Some(HELPER_TIMEOUT)).ok()?;
        stream.set_write_timeout(Some(HELPER_TIMEOUT)).ok()?;
        if !Self::root_peer(&stream) {
            return None;
        }
        stream.write_all(command.as_bytes()).ok()?;
        stream.write_all(b"\n").ok()?;
        stream.flush().ok()?;

        let mut response = String::new();
        BufReader::new(stream).take(HELPER_RESPONSE_LIMIT).read_line(&mut response).ok()?;
        let response = response.trim_end_matches(['\r', '\n']);
        (!response.is_empty()).then(|| response.to_owned())
    }
}

impl FreezeHelperClient for UnixFreezeHelper {
    fn is_available(&self) -> bool {
        self.command("ping").as_deref() == Some("PONG")
    }

    fn set_suspended(&self, pid: u32, suspended: bool) -> bool {
        let state = i32::from(suspended);
        self.command(&format!("freeze {pid} {state}")).as_deref() == Some("OK")
    }
}

/// Linux process suspension backend matching the current C++ proc/cgroup strategy.
pub struct ProcCgroupProcessControl {
    proc_root: PathBuf,
    cgroup_root: PathBuf,
    signal_sender: Arc<dyn SignalSender>,
    helper: Arc<dyn FreezeHelperClient>,
    authorization: Arc<dyn FreezeHelperAuthorization>,
}

impl fmt::Debug for ProcCgroupProcessControl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProcCgroupProcessControl")
            .field("proc_root", &self.proc_root)
            .field("cgroup_root", &self.cgroup_root)
            .finish_non_exhaustive()
    }
}

impl Default for ProcCgroupProcessControl {
    fn default() -> Self {
        Self {
            proc_root: PathBuf::from("/proc"),
            cgroup_root: PathBuf::from("/sys/fs/cgroup"),
            signal_sender: Arc::new(LibcSignalSender),
            helper: Arc::new(UnixFreezeHelper::default()),
            authorization: Arc::new(DenyFreezeHelperAuthorization),
        }
    }
}

impl ProcCgroupProcessControl {
    #[must_use]
    pub fn new(proc_root: impl Into<PathBuf>, cgroup_root: impl Into<PathBuf>) -> Self {
        Self { proc_root: proc_root.into(), cgroup_root: cgroup_root.into(), ..Self::default() }
    }

    #[must_use]
    pub fn with_helper(
        mut self,
        helper: Arc<dyn FreezeHelperClient>,
        authorization: Arc<dyn FreezeHelperAuthorization>,
    ) -> Self {
        self.helper = helper;
        self.authorization = authorization;
        self
    }

    fn process_exists(&self, pid: u32) -> bool {
        if pid == 0 {
            return false;
        }
        match self.signal_sender.send(pid, 0) {
            Ok(()) => true,
            Err(error) => error == libc::EPERM,
        }
    }

    fn tracer_pid(&self, pid: u32) -> Option<u32> {
        let contents =
            fs::read_to_string(self.proc_root.join(pid.to_string()).join("status")).ok()?;
        contents.lines().find_map(|line| {
            let value = line.strip_prefix("TracerPid:")?.trim();
            value.parse::<u32>().ok()
        })
    }

    fn is_ptraced(&self, pid: u32) -> bool {
        self.tracer_pid(pid).is_some_and(|tracer| tracer != 0)
    }

    fn cgroup_v2_available(&self) -> bool {
        self.cgroup_root.join("cgroup.controllers").exists()
    }

    fn cgroup_path(&self, pid: u32) -> Option<PathBuf> {
        let contents =
            fs::read_to_string(self.proc_root.join(pid.to_string()).join("cgroup")).ok()?;
        let relative = contents.lines().find_map(|line| line.strip_prefix("0::/"))?;
        Some(self.cgroup_root.join(relative))
    }

    fn process_tree(&self, root_pid: u32) -> Vec<u32> {
        if !self.process_exists(root_pid) {
            return Vec::new();
        }
        let records = ProcDiscovery::new(&self.proc_root).snapshot().unwrap_or_default();
        process_tree_from_records(root_pid, &records)
    }

    fn cgroup_operation(current: &Path, pid: u32, suspend: bool) -> io::Result<()> {
        let already_child = is_smu_freeze_cgroup(current);
        let (parent, child) = if already_child {
            let parent = current.parent().unwrap_or(current).to_path_buf();
            (parent, current.to_path_buf())
        } else {
            (current.to_path_buf(), current.join(FREEZE_GROUP_NAME))
        };

        if suspend {
            match fs::create_dir(&child) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
            fs::write(child.join("cgroup.procs"), pid.to_string())?;
            fs::write(child.join("cgroup.freeze"), "1")?;
            return Ok(());
        }

        fs::write(child.join("cgroup.freeze"), "0")?;
        fs::write(parent.join("cgroup.procs"), pid.to_string())?;
        let _ = fs::remove_dir(&child);
        Ok(())
    }

    fn try_helper(&self, pid: u32, suspend: bool) -> bool {
        if self.helper.set_suspended(pid, suspend) {
            return true;
        }
        if self.helper.is_available() || !self.authorization.wait_until_authorized() {
            return false;
        }
        self.helper.set_suspended(pid, suspend)
    }

    fn set_suspended(&self, pid: u32, suspend: bool) -> bool {
        if !self.process_exists(pid) {
            return false;
        }

        let ptraced = self.is_ptraced(pid);
        if self.cgroup_v2_available() {
            if let Some(cgroup_path) = self.cgroup_path(pid) {
                let should_use_cgroup = ptraced
                    || is_safe_app_cgroup(&cgroup_path)
                    || is_smu_freeze_cgroup(&cgroup_path);
                if should_use_cgroup {
                    match Self::cgroup_operation(&cgroup_path, pid, suspend) {
                        Ok(()) => return true,
                        Err(error) => {
                            let raw_error = error.raw_os_error().unwrap_or_default();
                            let helper_eligible = ptraced || is_smu_freeze_cgroup(&cgroup_path);
                            if should_try_helper(raw_error, helper_eligible)
                                && self.try_helper(pid, suspend)
                            {
                                return true;
                            }
                            if ptraced && suspend {
                                return false;
                            }
                        }
                    }
                }
            }
        } else if ptraced && suspend {
            return false;
        }

        let mut tree = self.process_tree(pid);
        if tree.is_empty() {
            tree.push(pid);
        }
        if suspend {
            tree.reverse();
        }
        let signal = if suspend { libc::SIGSTOP } else { libc::SIGCONT };
        let mut any_success = false;
        for target in tree {
            if self.signal_sender.send(target, signal).is_ok() {
                any_success = true;
            }
        }
        any_success
    }
}

impl ProcessControl for ProcCgroupProcessControl {
    fn suspend(&self, pid: u32) -> bool {
        self.set_suspended(pid, true)
    }

    fn resume(&self, pid: u32) -> bool {
        self.set_suspended(pid, false)
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

fn is_safe_app_cgroup(path: &Path) -> bool {
    let text = path.to_string_lossy();
    text.contains("app-") || text.contains("snap.") || text.contains("/app.slice/")
}

fn is_smu_freeze_cgroup(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == FREEZE_GROUP_NAME)
}

const fn should_try_helper(raw_error: i32, helper_eligible: bool) -> bool {
    helper_eligible && (raw_error == libc::EACCES || raw_error == libc::EPERM)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Debug, Default)]
    struct FakeSignalSender {
        existing: Mutex<Vec<u32>>,
        events: Mutex<Vec<(u32, i32)>>,
    }

    impl FakeSignalSender {
        fn with_existing(pids: &[u32]) -> Self {
            Self { existing: Mutex::new(pids.to_vec()), events: Mutex::new(Vec::new()) }
        }

        fn events(&self) -> Vec<(u32, i32)> {
            self.events.lock().expect("signal event lock").clone()
        }
    }

    impl SignalSender for FakeSignalSender {
        fn send(&self, pid: u32, signal: i32) -> Result<(), i32> {
            if !self.existing.lock().expect("existing pid lock").contains(&pid) {
                return Err(libc::ESRCH);
            }
            if signal != 0 {
                self.events.lock().expect("signal event lock").push((pid, signal));
            }
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct FakeHelper {
        available: bool,
        result: bool,
        events: Mutex<Vec<(u32, bool)>>,
    }

    impl FreezeHelperClient for FakeHelper {
        fn is_available(&self) -> bool {
            self.available
        }

        fn set_suspended(&self, pid: u32, suspended: bool) -> bool {
            self.events.lock().expect("helper event lock").push((pid, suspended));
            self.result
        }
    }

    #[derive(Debug)]
    struct FakeAuthorization(bool);

    impl FreezeHelperAuthorization for FakeAuthorization {
        fn wait_until_authorized(&self) -> bool {
            self.0
        }
    }

    fn temp_roots(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("smu-process-control-{name}-{nonce}"));
        let proc_root = root.join("proc");
        let cgroup_root = root.join("cgroup");
        fs::create_dir_all(&proc_root).expect("fake proc root should be created");
        fs::create_dir_all(&cgroup_root).expect("fake cgroup root should be created");
        (root, proc_root, cgroup_root)
    }

    fn write_proc(proc_root: &Path, pid: u32, parent: u32, tracer: u32, cgroup: &str) {
        let process = proc_root.join(pid.to_string());
        fs::create_dir_all(&process).expect("fake process should be created");
        fs::write(process.join("comm"), format!("p{pid}\n")).expect("comm should be written");
        fs::write(process.join("stat"), format!("{pid} (p{pid}) S {parent} 0 0 0\n"))
            .expect("stat should be written");
        fs::write(process.join("status"), format!("TracerPid:\t{tracer}\n"))
            .expect("status should be written");
        fs::write(process.join("cgroup"), format!("0::/{cgroup}\n"))
            .expect("cgroup should be written");
    }

    fn control_with_signals(
        proc_root: &Path,
        cgroup_root: &Path,
        sender: Arc<FakeSignalSender>,
    ) -> ProcCgroupProcessControl {
        ProcCgroupProcessControl {
            proc_root: proc_root.to_path_buf(),
            cgroup_root: cgroup_root.to_path_buf(),
            signal_sender: sender,
            helper: Arc::new(FakeHelper::default()),
            authorization: Arc::new(FakeAuthorization(false)),
        }
    }

    #[test]
    fn process_tree_matches_cpp_depth_first_frontier_shape() {
        let records = [
            ProcessRecord {
                pid: 10,
                parent_pid: Some(1),
                command_name: None,
                executable_name: None,
            },
            ProcessRecord {
                pid: 11,
                parent_pid: Some(10),
                command_name: None,
                executable_name: None,
            },
            ProcessRecord {
                pid: 12,
                parent_pid: Some(10),
                command_name: None,
                executable_name: None,
            },
            ProcessRecord {
                pid: 13,
                parent_pid: Some(12),
                command_name: None,
                executable_name: None,
            },
        ];
        assert_eq!(process_tree_from_records(10, &records), [10, 11, 12, 13]);
        assert!(process_tree_from_records(99, &records).is_empty());
    }

    #[test]
    fn signal_fallback_suspends_children_before_parent_and_resumes_parent_first() {
        let (root, proc_root, cgroup_root) = temp_roots("signals");
        write_proc(&proc_root, 10, 1, 0, "plain.scope");
        write_proc(&proc_root, 11, 10, 0, "plain.scope");
        write_proc(&proc_root, 12, 10, 0, "plain.scope");
        let sender = Arc::new(FakeSignalSender::with_existing(&[10, 11, 12]));
        let control = control_with_signals(&proc_root, &cgroup_root, sender.clone());

        assert!(control.suspend(10));
        assert_eq!(
            sender.events(),
            [(12, libc::SIGSTOP), (11, libc::SIGSTOP), (10, libc::SIGSTOP)]
        );
        sender.events.lock().expect("signal event lock").clear();
        assert!(control.resume(10));
        assert_eq!(
            sender.events(),
            [(10, libc::SIGCONT), (11, libc::SIGCONT), (12, libc::SIGCONT)]
        );
        fs::remove_dir_all(root).expect("fixture should be removable");
    }

    #[test]
    fn safe_app_cgroup_uses_child_freezer_before_signals() {
        let (root, proc_root, cgroup_root) = temp_roots("cgroup");
        fs::write(cgroup_root.join("cgroup.controllers"), "cpu memory")
            .expect("controller marker should be written");
        let current = cgroup_root.join("user.slice/app-test.scope");
        fs::create_dir_all(&current).expect("fake current cgroup should be created");
        write_proc(&proc_root, 20, 1, 0, "user.slice/app-test.scope");
        let sender = Arc::new(FakeSignalSender::with_existing(&[20]));
        let control = control_with_signals(&proc_root, &cgroup_root, sender.clone());

        assert!(control.suspend(20));
        let child = current.join(FREEZE_GROUP_NAME);
        assert_eq!(
            fs::read_to_string(child.join("cgroup.procs")).expect("child procs should exist"),
            "20"
        );
        assert_eq!(
            fs::read_to_string(child.join("cgroup.freeze")).expect("freeze state should exist"),
            "1"
        );
        assert!(sender.events().is_empty());

        fs::write(proc_root.join("20/cgroup"), "0::/user.slice/app-test.scope/smu_freeze\n")
            .expect("fake moved cgroup should be updated");
        assert!(control.resume(20));
        assert_eq!(
            fs::read_to_string(child.join("cgroup.freeze")).expect("thaw state should exist"),
            "0"
        );
        assert_eq!(
            fs::read_to_string(current.join("cgroup.procs")).expect("parent procs should exist"),
            "20"
        );
        assert!(sender.events().is_empty());
        fs::remove_dir_all(root).expect("fixture should be removable");
    }

    #[test]
    fn ptraced_process_refuses_sigstop_when_cgroup_v2_is_unavailable() {
        let (root, proc_root, cgroup_root) = temp_roots("ptraced");
        write_proc(&proc_root, 30, 1, 99, "plain.scope");
        let sender = Arc::new(FakeSignalSender::with_existing(&[30]));
        let control = control_with_signals(&proc_root, &cgroup_root, sender.clone());

        assert!(!control.suspend(30));
        assert!(sender.events().is_empty());
        fs::remove_dir_all(root).expect("fixture should be removable");
    }

    #[test]
    fn helper_eligibility_matches_cpp_permission_rule() {
        assert!(should_try_helper(libc::EACCES, true));
        assert!(should_try_helper(libc::EPERM, true));
        assert!(!should_try_helper(libc::ENOENT, true));
        assert!(!should_try_helper(libc::EACCES, false));
    }

    #[test]
    fn cgroup_path_classification_matches_cpp_strings() {
        assert!(is_safe_app_cgroup(Path::new("/sys/fs/cgroup/user.slice/app-foo.scope")));
        assert!(is_safe_app_cgroup(Path::new("/sys/fs/cgroup/snap.foo.scope")));
        assert!(is_safe_app_cgroup(Path::new("/sys/fs/cgroup/app.slice/game")));
        assert!(!is_safe_app_cgroup(Path::new("/sys/fs/cgroup/user.slice/plain.scope")));
        assert!(is_smu_freeze_cgroup(Path::new(
            "/sys/fs/cgroup/user.slice/app-foo.scope/smu_freeze"
        )));
    }

    #[test]
    fn unavailable_pid_short_circuits_without_signals() {
        let (root, proc_root, cgroup_root) = temp_roots("missing");
        let sender = Arc::new(FakeSignalSender::default());
        let control = control_with_signals(&proc_root, &cgroup_root, sender.clone());
        assert!(!control.suspend(1234));
        assert!(sender.events().is_empty());
        fs::remove_dir_all(root).expect("fixture should be removable");
    }
}
