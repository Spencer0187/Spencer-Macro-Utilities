use smu_network::linux::{NetHelperClient, UnixNetHelperClient};
use smu_process::linux::{FreezeHelperClient, UnixFreezeHelper};
use std::ffi::CStr;
use std::fs::{self, File, Permissions};
use std::io::{self, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

const START_TIMEOUT: Duration = Duration::from_secs(120);
const START_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HelperKind {
    Network,
    Process,
}

impl HelperKind {
    const fn binary_name(self) -> &'static str {
        match self {
            Self::Network => "nethelper",
            Self::Process => "processhelper",
        }
    }

    const fn stage_template(self) -> &'static [u8] {
        match self {
            Self::Network => b"/tmp/smu-nethelper-stage-XXXXXX\0",
            Self::Process => b"/tmp/smu-processhelper-stage-XXXXXX\0",
        }
    }

    const fn direct_when_root(self) -> bool {
        matches!(self, Self::Network)
    }

    const fn include_cwd_candidate(self) -> bool {
        matches!(self, Self::Network)
    }

    const fn identity_error(self) -> &'static str {
        match self {
            Self::Network => {
                "Could not read the SMU process identity required to safely start the Linux lagswitch helper."
            }
            Self::Process => {
                "Could not read the SMU process identity required to safely start the Linux process helper."
            }
        }
    }

    fn missing_error(self, path: &Path) -> String {
        match self {
            Self::Network => format!("Linux lagswitch helper was not found at {}.", path.display()),
            Self::Process => format!("Linux process helper was not found at {}.", path.display()),
        }
    }

    fn stage_create_error(self, error: &io::Error) -> String {
        match self {
            Self::Network => {
                format!("Could not create a private Linux lagswitch helper staging file: {error}.")
            }
            Self::Process => format!("Could not stage Linux process helper: {error}."),
        }
    }

    const fn stage_protect_error(self) -> &'static str {
        match self {
            Self::Network => "Could not protect the staged Linux lagswitch helper.",
            Self::Process => "Could not protect staged Linux process helper.",
        }
    }

    fn stage_read_error(self, path: &Path) -> String {
        match self {
            Self::Network => {
                format!("Could not read Linux lagswitch helper at {}.", path.display())
            }
            Self::Process => "Could not read Linux process helper.".to_owned(),
        }
    }

    fn stage_copy_error(self, error: &io::Error) -> String {
        match self {
            Self::Network => format!("Could not stage Linux lagswitch helper: {error}."),
            Self::Process => "Could not copy Linux process helper.".to_owned(),
        }
    }

    fn stage_finish_error(self, error: &io::Error) -> String {
        match self {
            Self::Network => format!("Could not finish staging Linux lagswitch helper: {error}."),
            Self::Process => "Could not finish staging Linux process helper.".to_owned(),
        }
    }

    const fn pkexec_missing_error(self) -> &'static str {
        match self {
            Self::Network => {
                "pkexec was not found. Install polkit or start nethelper manually with elevated permissions."
            }
            Self::Process => {
                "pkexec was not found. Install polkit to use the privileged freeze fallback."
            }
        }
    }

    fn spawn_error(self, error: &io::Error, using_pkexec: bool) -> String {
        if using_pkexec && error.kind() == io::ErrorKind::NotFound {
            return self.pkexec_missing_error().to_owned();
        }
        match self {
            Self::Network => format!("Failed to fork Linux lagswitch helper: {error}."),
            Self::Process => format!("Failed to fork Linux process helper: {error}."),
        }
    }

    const fn exited_error(self, code: Option<i32>) -> &'static str {
        match self {
            Self::Network => match code {
                Some(127) => self.pkexec_missing_error(),
                Some(126) => "Linux lagswitch helper could not be launched.",
                _ => "Linux lagswitch helper permission prompt was cancelled or denied.",
            },
            Self::Process => match code {
                Some(127) => self.pkexec_missing_error(),
                _ => {
                    "Linux process helper permission prompt was cancelled, denied, or the helper failed to start."
                }
            },
        }
    }

    const fn monitor_error(self) -> &'static str {
        match self {
            Self::Network => "Could not monitor Linux lagswitch helper startup",
            Self::Process => "Could not monitor Linux process helper startup",
        }
    }

    const fn timeout_error(self) -> &'static str {
        match self {
            Self::Network => "Timed out waiting for Linux lagswitch helper authentication.",
            Self::Process => "Timed out waiting for Linux process helper authentication.",
        }
    }
}

#[derive(Debug)]
struct StagedHelper {
    path: PathBuf,
}

impl Drop for StagedHelper {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn executable_directory() -> Option<PathBuf> {
    fs::read_link("/proc/self/exe").ok().and_then(|path| path.parent().map(Path::to_path_buf))
}

fn app_dir_candidates() -> Vec<PathBuf> {
    ["SMU_APPDIR", "APPDIR"]
        .into_iter()
        .filter_map(std::env::var_os)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn resolve_helper_path_from(
    kind: HelperKind,
    executable_dir: &Path,
    app_dirs: &[PathBuf],
    current_dir: &Path,
) -> PathBuf {
    let name = kind.binary_name();
    for candidate in [executable_dir.join(name), executable_dir.join("usr/bin").join(name)] {
        if candidate.exists() {
            return candidate;
        }
    }

    for app_dir in app_dirs {
        for candidate in [app_dir.join("usr/bin").join(name), app_dir.join(name)] {
            if candidate.exists() {
                return candidate;
            }
        }
    }

    if kind.include_cwd_candidate() {
        let candidate = current_dir.join(name);
        if candidate.exists() {
            return candidate;
        }
    }
    executable_dir.join(name)
}

fn resolve_helper_path(kind: HelperKind) -> PathBuf {
    let executable_dir = executable_directory().unwrap_or_default();
    let current_dir = std::env::current_dir().unwrap_or_default();
    resolve_helper_path_from(kind, &executable_dir, &app_dir_candidates(), &current_dir)
}

fn parse_process_start_time(stat: &str) -> Option<u64> {
    let close_name = stat.rfind(')')?;
    let mut fields = stat.get(close_name + 1..)?.split_ascii_whitespace();
    let mut field_22 = None;
    for field in 3..=22 {
        let value = fields.next()?;
        if field == 22 {
            field_22 = value.parse::<u64>().ok();
        }
    }
    field_22.filter(|value| *value != 0)
}

fn read_process_start_time(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    parse_process_start_time(&stat)
}

fn mkstemp(template: &[u8]) -> io::Result<(File, PathBuf)> {
    let mut buffer = template.to_vec();
    if buffer.last() != Some(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "mkstemp template must be NUL-terminated",
        ));
    }
    // SAFETY: `buffer` is mutable, NUL-terminated, and lives through the call. `mkstemp` replaces
    // the trailing X characters in-place and returns an owned file descriptor on success.
    let fd: RawFd = unsafe { libc::mkstemp(buffer.as_mut_ptr().cast()) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful `mkstemp` returned a fresh owned fd that is transferred exactly once.
    let file = unsafe { File::from_raw_fd(fd) };
    // SAFETY: successful `mkstemp` leaves a valid NUL-terminated path in the same buffer.
    let path = unsafe { CStr::from_ptr(buffer.as_ptr().cast()) };
    Ok((file, PathBuf::from(path.to_string_lossy().into_owned())))
}

fn stage_helper(kind: HelperKind, source_path: &Path) -> Result<StagedHelper, String> {
    if !source_path.exists() {
        return Err(kind.missing_error(source_path));
    }
    let (mut destination, staged_path) =
        mkstemp(kind.stage_template()).map_err(|error| kind.stage_create_error(&error))?;
    let cleanup = || {
        let _ = fs::remove_file(&staged_path);
    };
    if destination.set_permissions(Permissions::from_mode(0o700)).is_err() {
        cleanup();
        return Err(kind.stage_protect_error().to_owned());
    }
    let mut source = File::open(source_path).map_err(|_| {
        cleanup();
        kind.stage_read_error(source_path)
    })?;
    if let Err(error) = io::copy(&mut source, &mut destination) {
        cleanup();
        return Err(kind.stage_copy_error(&error));
    }
    if let Err(error) = destination.flush().and_then(|()| destination.sync_all()) {
        cleanup();
        return Err(kind.stage_finish_error(&error));
    }
    drop(destination);
    Ok(StagedHelper { path: staged_path })
}

fn spawn_helper(
    kind: HelperKind,
    staged: &Path,
    owner_pid: u32,
    start_time: u64,
) -> Result<Child, String> {
    // SAFETY: `geteuid` has no preconditions and touches no caller-owned memory.
    let root = unsafe { libc::geteuid() } == 0;
    // SAFETY: `getuid` has no preconditions and touches no caller-owned memory.
    let uid = unsafe { libc::getuid() };
    let direct = root && kind.direct_when_root();
    let mut command = if direct {
        Command::new(staged)
    } else {
        let mut command = Command::new("pkexec");
        command.arg("--disable-internal-agent").arg(staged);
        command
    };
    command
        .arg("--uid")
        .arg(uid.to_string())
        .arg("--owner-pid")
        .arg(owner_pid.to_string())
        .arg("--owner-start-time")
        .arg(start_time.to_string());
    command.spawn().map_err(|error| kind.spawn_error(&error, !direct))
}

fn exit_status_error(kind: HelperKind, status: ExitStatus) -> String {
    kind.exited_error(status.code()).to_owned()
}

fn wait_for_helper(
    kind: HelperKind,
    child: &mut Child,
    reachable: &dyn Fn() -> bool,
) -> Result<(), String> {
    let deadline = Instant::now() + START_TIMEOUT;
    while Instant::now() < deadline {
        if reachable() {
            return Ok(());
        }
        match child.try_wait() {
            Ok(Some(status)) => return Err(exit_status_error(kind, status)),
            Ok(None) => std::thread::sleep(START_POLL_INTERVAL),
            Err(error) => return Err(format!("{}: {error}.", kind.monitor_error())),
        }
    }
    Err(kind.timeout_error().to_owned())
}

fn reap_in_background(mut child: Child) {
    let _ = std::thread::spawn(move || {
        let _ = child.wait();
    });
}

fn start_helper(kind: HelperKind, reachable: &dyn Fn() -> bool) -> Result<(), String> {
    if reachable() {
        return Ok(());
    }

    let owner_pid = std::process::id();
    let start_time =
        read_process_start_time(owner_pid).ok_or_else(|| kind.identity_error().to_owned())?;
    let source = resolve_helper_path(kind);
    let staged = stage_helper(kind, &source)?;
    let mut child = spawn_helper(kind, &staged.path, owner_pid, start_time)?;
    let result = wait_for_helper(kind, &mut child, reachable);
    if child.try_wait().ok().flatten().is_none() {
        reap_in_background(child);
    }
    result
}

pub fn start_linux_network_helper_with_graphical_pkexec() -> Result<(), String> {
    let client = UnixNetHelperClient::default();
    start_helper(HelperKind::Network, &|| client.send_command("ping").as_deref() == Some("PONG"))
}

pub fn start_linux_process_helper_with_graphical_pkexec() -> Result<(), String> {
    let client = UnixFreezeHelper::default();
    start_helper(HelperKind::Process, &|| client.is_available())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("smu-helper-launch-{name}-{nonce}"));
        fs::create_dir_all(&root).expect("fixture directory should be created");
        root
    }

    #[test]
    fn process_start_time_parser_handles_spaces_and_parentheses_in_comm() {
        let mut fields = vec!["S".to_owned()];
        fields.extend((4..=21).map(|value| value.to_string()));
        fields.push("987654321".to_owned());
        let stat = format!("42 (weird ) helper name) {}", fields.join(" "));
        assert_eq!(parse_process_start_time(&stat), Some(987_654_321));
        assert_eq!(parse_process_start_time("42 (x) S 1"), None);
    }

    #[test]
    fn network_path_resolution_matches_cpp_candidate_order() {
        let root = temp_dir("network-path");
        let exe = root.join("exe");
        let app = root.join("app");
        let cwd = root.join("cwd");
        fs::create_dir_all(exe.join("usr/bin")).expect("exe tree");
        fs::create_dir_all(app.join("usr/bin")).expect("app tree");
        fs::create_dir_all(&cwd).expect("cwd tree");
        fs::write(cwd.join("nethelper"), b"cwd").expect("cwd helper");
        assert_eq!(
            resolve_helper_path_from(HelperKind::Network, &exe, std::slice::from_ref(&app), &cwd),
            cwd.join("nethelper")
        );
        fs::write(app.join("usr/bin/nethelper"), b"app").expect("app helper");
        assert_eq!(
            resolve_helper_path_from(HelperKind::Network, &exe, std::slice::from_ref(&app), &cwd),
            app.join("usr/bin/nethelper")
        );
        fs::remove_dir_all(root).expect("fixture should be removable");
    }

    #[test]
    fn process_path_resolution_does_not_use_cwd_fallback() {
        let root = temp_dir("process-path");
        let exe = root.join("exe");
        let cwd = root.join("cwd");
        fs::create_dir_all(&exe).expect("exe tree");
        fs::create_dir_all(&cwd).expect("cwd tree");
        fs::write(cwd.join("processhelper"), b"cwd").expect("cwd helper");
        assert_eq!(
            resolve_helper_path_from(HelperKind::Process, &exe, &[], &cwd),
            exe.join("processhelper")
        );
        fs::remove_dir_all(root).expect("fixture should be removable");
    }

    #[test]
    fn staging_copies_bytes_and_uses_private_executable_permissions() {
        let root = temp_dir("stage");
        let source = root.join("nethelper");
        fs::write(&source, b"helper bytes").expect("helper should be written");
        let staged = stage_helper(HelperKind::Network, &source).expect("helper should stage");
        assert_eq!(fs::read(&staged.path).expect("staged bytes should read"), b"helper bytes");
        let mode =
            fs::metadata(&staged.path).expect("staged metadata should exist").permissions().mode()
                & 0o777;
        assert_eq!(mode, 0o700);
        let staged_path = staged.path.clone();
        drop(staged);
        assert!(!staged_path.exists());
        fs::remove_dir_all(root).expect("fixture should be removable");
    }

    #[test]
    fn exit_messages_match_both_cpp_launchers() {
        assert_eq!(
            HelperKind::Network.exited_error(Some(127)),
            "pkexec was not found. Install polkit or start nethelper manually with elevated permissions."
        );
        assert_eq!(
            HelperKind::Network.exited_error(Some(126)),
            "Linux lagswitch helper could not be launched."
        );
        assert_eq!(
            HelperKind::Process.exited_error(Some(127)),
            "pkexec was not found. Install polkit to use the privileged freeze fallback."
        );
        assert_eq!(
            HelperKind::Process.exited_error(Some(1)),
            "Linux process helper permission prompt was cancelled, denied, or the helper failed to start."
        );
    }
}
