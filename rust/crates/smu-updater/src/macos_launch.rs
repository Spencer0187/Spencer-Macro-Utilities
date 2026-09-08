use crate::macos::{path_has_extended_acl, require_matching_macos_release_signer};
use crate::{is_canonical_manifest_version, macos_update_bundle_if_replaceable};
use sha2::{Digest as _, Sha256};
use std::ffi::CString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HELPER_NAME: &str = "SMU-Update-Helper";
const TEMP_ATTEMPTS: usize = 128;
const MAXIMUM_HELPER_BYTES: u64 = 64 * 1024 * 1024;
const READY_TIMEOUT: Duration = Duration::from_secs(180);
const READY_POLL_MS: libc::c_int = 50;
const HANDOFF_SURVIVAL: Duration = Duration::from_millis(100);
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MacosUpdateLaunchError(String);

impl MacosUpdateLaunchError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for MacosUpdateLaunchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for MacosUpdateLaunchError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MacosUpdateLaunch {
    pub helper_pid: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

fn directory_identity(file: &File) -> Result<FileIdentity, MacosUpdateLaunchError> {
    let metadata = file.metadata().map_err(|error| {
        MacosUpdateLaunchError::new(format!(
            "Could not inspect pinned macOS update directory: {error}"
        ))
    })?;
    if !metadata.is_dir() {
        return Err(MacosUpdateLaunchError::new(
            "Pinned macOS update directory is not a directory.",
        ));
    }
    Ok(FileIdentity { device: metadata.dev(), inode: metadata.ino() })
}

fn require_directory_path_identity(
    path: &Path,
    guard: &File,
) -> Result<(), MacosUpdateLaunchError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        MacosUpdateLaunchError::new(format!(
            "Could not inspect detached macOS update directory path: {error}"
        ))
    })?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || (FileIdentity { device: metadata.dev(), inode: metadata.ino() })
            != directory_identity(guard)?
    {
        return Err(MacosUpdateLaunchError::new(
            "Detached macOS update directory changed identity.",
        ));
    }
    Ok(())
}

fn file_identity(file: &File) -> Result<FileIdentity, MacosUpdateLaunchError> {
    let metadata = file.metadata().map_err(|error| {
        MacosUpdateLaunchError::new(format!("Could not inspect pinned macOS update file: {error}"))
    })?;
    Ok(FileIdentity { device: metadata.dev(), inode: metadata.ino() })
}

fn path_identity(path: &Path) -> Result<FileIdentity, MacosUpdateLaunchError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        MacosUpdateLaunchError::new(format!("Could not inspect macOS update path: {error}"))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(MacosUpdateLaunchError::new(
            "macOS updater refused a non-regular update file path.",
        ));
    }
    Ok(FileIdentity { device: metadata.dev(), inode: metadata.ino() })
}

fn open_pinned_regular_file(path: &Path, label: &str) -> Result<File, MacosUpdateLaunchError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| {
            MacosUpdateLaunchError::new(format!("Could not open and pin {label}: {error}"))
        })?;
    let metadata = file.metadata().map_err(|error| {
        MacosUpdateLaunchError::new(format!("Could not inspect pinned {label}: {error}"))
    })?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAXIMUM_HELPER_BYTES {
        return Err(MacosUpdateLaunchError::new(format!(
            "macOS updater refused an invalid or oversized {label}."
        )));
    }
    Ok(file)
}

fn read_pinned_file(file: &mut File, label: &str) -> Result<Vec<u8>, MacosUpdateLaunchError> {
    file.seek(SeekFrom::Start(0)).map_err(|error| {
        MacosUpdateLaunchError::new(format!("Could not rewind {label}: {error}"))
    })?;
    let size = file
        .metadata()
        .map_err(|error| {
            MacosUpdateLaunchError::new(format!("Could not inspect {label}: {error}"))
        })?
        .len();
    if size == 0 || size > MAXIMUM_HELPER_BYTES {
        return Err(MacosUpdateLaunchError::new(format!(
            "macOS updater refused an invalid or oversized {label}."
        )));
    }
    let capacity = usize::try_from(size)
        .map_err(|_| MacosUpdateLaunchError::new(format!("{label} was too large to copy.")))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(MAXIMUM_HELPER_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| MacosUpdateLaunchError::new(format!("Could not read {label}: {error}")))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != size {
        return Err(MacosUpdateLaunchError::new(format!(
            "{label} changed while it was being copied."
        )));
    }
    Ok(bytes)
}

fn open_pinned_directory(path: &Path) -> Result<File, MacosUpdateLaunchError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| {
            MacosUpdateLaunchError::new(format!(
                "Could not pin detached macOS update directory: {error}"
            ))
        })?;
    // SAFETY: file is a live directory descriptor owned by this process.
    if unsafe { libc::fchmod(file.as_raw_fd(), 0o700) } != 0 {
        return Err(MacosUpdateLaunchError::new(format!(
            "Could not set detached macOS update directory permissions: {}",
            std::io::Error::last_os_error()
        )));
    }
    let metadata = file.metadata().map_err(|error| {
        MacosUpdateLaunchError::new(format!(
            "Could not inspect detached macOS update directory: {error}"
        ))
    })?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o777 != 0o700
    {
        return Err(MacosUpdateLaunchError::new(
            "macOS updater refused a non-private detached update directory.",
        ));
    }
    Ok(file)
}

fn clear_extended_acl(path: &Path) -> Result<(), MacosUpdateLaunchError> {
    let status = Command::new("/bin/chmod").arg("-N").arg(path).status().map_err(|error| {
        MacosUpdateLaunchError::new(format!(
            "Could not launch chmod for detached macOS update directory: {error}"
        ))
    })?;
    if !status.success()
        || path_has_extended_acl(path)
            .map_err(|error| MacosUpdateLaunchError::new(error.to_string()))?
    {
        return Err(MacosUpdateLaunchError::new(
            "Could not establish an ACL-free detached macOS update directory.",
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct DetachedLaunchDirectory {
    path: PathBuf,
    guard: File,
    retain: bool,
}

impl DetachedLaunchDirectory {
    fn create(old_pid: libc::pid_t) -> Result<Self, MacosUpdateLaunchError> {
        let base = fs::canonicalize(std::env::temp_dir()).map_err(|error| {
            MacosUpdateLaunchError::new(format!(
                "Could not resolve the macOS temporary directory: {error}"
            ))
        })?;
        let epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        for _ in 0..TEMP_ATTEMPTS {
            let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("smu-update-launch-{old_pid}-{epoch}-{counter}"));
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(&path) {
                Ok(()) => {
                    let result = (|| {
                        clear_extended_acl(&path)?;
                        let canonical = fs::canonicalize(&path).map_err(|error| {
                            MacosUpdateLaunchError::new(format!(
                                "Could not resolve detached macOS update directory: {error}"
                            ))
                        })?;
                        if canonical.parent() != Some(base.as_path()) {
                            return Err(MacosUpdateLaunchError::new(
                                "Detached macOS update directory escaped the temporary root.",
                            ));
                        }
                        let guard = open_pinned_directory(&canonical)?;
                        Ok(Self { path: canonical, guard, retain: false })
                    })();
                    // If validation fails, leave the empty private root rather than reopening a pathname rmdir.
                    return result;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(MacosUpdateLaunchError::new(format!(
                        "Could not create detached macOS update directory: {error}"
                    )));
                }
            }
        }
        Err(MacosUpdateLaunchError::new("Could not allocate a detached macOS update directory."))
    }

    fn retain(&mut self) {
        self.retain = true;
    }
}

impl Drop for DetachedLaunchDirectory {
    fn drop(&mut self) {
        if self.retain {
            return;
        }
        let Ok(name) = CString::new(HELPER_NAME) else {
            return;
        };
        // SAFETY: guard remains a live directory descriptor and name is the fixed NUL-terminated helper child name.
        let _ = unsafe { libc::unlinkat(self.guard.as_raw_fd(), name.as_ptr(), 0) };
        // macOS has no safe unlink-by-directory-handle primitive for the root. Deliberately retain the now-empty
        // private directory rather than racing a final pathname rmdir against a same-UID rename/substitution.
    }
}

fn write_detached_helper(directory: &File, bytes: &[u8]) -> Result<File, MacosUpdateLaunchError> {
    let name = CString::new(HELPER_NAME).map_err(|_| {
        MacosUpdateLaunchError::new("macOS update helper name contained an embedded NUL byte.")
    })?;
    // SAFETY: directory remains live and name is a valid fixed child name. O_EXCL/O_NOFOLLOW prevent adoption.
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o500,
        )
    };
    if fd < 0 {
        return Err(MacosUpdateLaunchError::new(format!(
            "Could not create detached macOS update helper: {}",
            std::io::Error::last_os_error()
        )));
    }
    // SAFETY: fd is newly owned after successful openat.
    let mut file = unsafe { File::from_raw_fd(fd) };
    // SAFETY: file is a live regular-file descriptor owned by this process.
    if unsafe { libc::fchmod(file.as_raw_fd(), 0o500) } != 0 {
        return Err(MacosUpdateLaunchError::new(format!(
            "Could not set detached macOS update helper permissions: {}",
            std::io::Error::last_os_error()
        )));
    }
    file.write_all(bytes).and_then(|()| file.sync_all()).map_err(|error| {
        MacosUpdateLaunchError::new(format!(
            "Could not write detached macOS update helper: {error}"
        ))
    })?;
    Ok(file)
}

fn stage_detached_helper(
    installed_bundle: &Path,
    installed_helper: &Path,
    directory: &DetachedLaunchDirectory,
) -> Result<(PathBuf, File), MacosUpdateLaunchError> {
    let mut installed =
        open_pinned_regular_file(installed_helper, "installed macOS update helper")?;
    let installed_identity = file_identity(&installed)?;
    require_matching_macos_release_signer(installed_bundle, installed_helper)
        .map_err(|error| MacosUpdateLaunchError::new(error.to_string()))?;
    if path_identity(installed_helper)? != installed_identity {
        return Err(MacosUpdateLaunchError::new(
            "Installed macOS update helper changed identity during signer validation.",
        ));
    }
    let installed_bytes = read_pinned_file(&mut installed, "installed macOS update helper")?;
    let installed_digest: [u8; 32] = Sha256::digest(&installed_bytes).into();

    require_directory_path_identity(&directory.path, &directory.guard)?;
    let detached = directory.path.join(HELPER_NAME);
    let mut detached_file = write_detached_helper(&directory.guard, &installed_bytes)?;
    let detached_identity = file_identity(&detached_file)?;
    let detached_bytes = read_pinned_file(&mut detached_file, "detached macOS update helper")?;
    let detached_digest: [u8; 32] = Sha256::digest(&detached_bytes).into();
    if detached_digest != installed_digest {
        return Err(MacosUpdateLaunchError::new(
            "Detached macOS update helper did not match the installed signed helper.",
        ));
    }
    require_directory_path_identity(&directory.path, &directory.guard)?;
    require_matching_macos_release_signer(installed_bundle, &detached)
        .map_err(|error| MacosUpdateLaunchError::new(error.to_string()))?;
    if path_identity(&detached)? != detached_identity {
        return Err(MacosUpdateLaunchError::new(
            "Detached macOS update helper changed identity during signer validation.",
        ));
    }

    // Do not keep a write-capable descriptor open across process spawn: Unix kernels may reject execution with
    // ETXTBSY while an executable is still open for writing. Close the O_RDWR staging handle, reopen O_RDONLY,
    // verify the same inode, and rehash the signer-validated bytes before handing the guard to the launcher.
    drop(detached_file);
    let mut detached_guard = open_pinned_regular_file(&detached, "detached macOS update helper")?;
    if file_identity(&detached_guard)? != detached_identity {
        return Err(MacosUpdateLaunchError::new(
            "Detached macOS update helper changed identity while reopening read-only.",
        ));
    }
    let verified_bytes = read_pinned_file(&mut detached_guard, "detached macOS update helper")?;
    let verified_digest: [u8; 32] = Sha256::digest(&verified_bytes).into();
    if verified_digest != installed_digest {
        return Err(MacosUpdateLaunchError::new(
            "Detached macOS update helper changed bytes after signer validation.",
        ));
    }
    if path_identity(&detached)? != detached_identity {
        return Err(MacosUpdateLaunchError::new(
            "Detached macOS update helper changed identity after reopening read-only.",
        ));
    }
    Ok((detached, detached_guard))
}

fn set_close_on_exec(fd: libc::c_int, enabled: bool) -> Result<(), MacosUpdateLaunchError> {
    // SAFETY: F_GETFD/F_SETFD operate on the supplied live descriptor without dereferencing pointers.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(MacosUpdateLaunchError::new(format!(
            "Could not inspect macOS updater readiness descriptor: {}",
            std::io::Error::last_os_error()
        )));
    }
    let desired = if enabled { flags | libc::FD_CLOEXEC } else { flags & !libc::FD_CLOEXEC };
    // SAFETY: F_SETFD updates descriptor flags for the supplied live descriptor.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, desired) } < 0 {
        return Err(MacosUpdateLaunchError::new(format!(
            "Could not configure macOS updater readiness descriptor: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

fn create_readiness_pipe() -> Result<(File, OwnedFd), MacosUpdateLaunchError> {
    let mut fds = [-1; 2];
    // SAFETY: fds points to storage for exactly two descriptors initialized by pipe on success.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(MacosUpdateLaunchError::new(format!(
            "Could not create macOS updater readiness pipe: {}",
            std::io::Error::last_os_error()
        )));
    }
    // SAFETY: both descriptors are newly owned after successful pipe().
    let read = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    // SAFETY: both descriptors are newly owned after successful pipe().
    let write = unsafe { OwnedFd::from_raw_fd(fds[1]) };
    set_close_on_exec(read.as_raw_fd(), true)?;
    set_close_on_exec(write.as_raw_fd(), false)?;
    Ok((File::from(read), write))
}

fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn wait_for_ready(child: &mut Child, read: &mut File) -> Result<(), MacosUpdateLaunchError> {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        let mut descriptor = libc::pollfd {
            fd: read.as_raw_fd(),
            events: libc::POLLIN | libc::POLLHUP | libc::POLLERR,
            revents: 0,
        };
        // SAFETY: descriptor points to one initialized pollfd and timeout is finite.
        let polled = unsafe { libc::poll(&raw mut descriptor, 1, READY_POLL_MS) };
        if polled < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            stop_child(child);
            return Err(MacosUpdateLaunchError::new(format!(
                "Could not wait for macOS updater readiness: {error}"
            )));
        }
        if polled > 0 && descriptor.revents & libc::POLLIN != 0 {
            let mut bytes = [0_u8; std::mem::size_of::<u32>()];
            read.read_exact(&mut bytes).map_err(|error| {
                MacosUpdateLaunchError::new(format!(
                    "Could not read macOS updater readiness signal: {error}"
                ))
            })?;
            let helper_pid = u32::from_ne_bytes(bytes);
            if helper_pid != child.id() {
                stop_child(child);
                return Err(MacosUpdateLaunchError::new(
                    "macOS updater readiness signal did not match the launched helper process.",
                ));
            }
            if let Some(status) = child.try_wait().map_err(|error| {
                MacosUpdateLaunchError::new(format!(
                    "Could not inspect detached macOS update helper: {error}"
                ))
            })? {
                return Err(MacosUpdateLaunchError::new(format!(
                    "Detached macOS update helper exited after signaling readiness with status {status}."
                )));
            }
            std::thread::sleep(HANDOFF_SURVIVAL);
            if let Some(status) = child.try_wait().map_err(|error| {
                MacosUpdateLaunchError::new(format!(
                    "Could not inspect detached macOS update helper after readiness: {error}"
                ))
            })? {
                return Err(MacosUpdateLaunchError::new(format!(
                    "Detached macOS update helper did not survive the handoff window: {status}."
                )));
            }
            return Ok(());
        }
        if polled > 0 && descriptor.revents & (libc::POLLHUP | libc::POLLERR) != 0 {
            if let Some(status) = child.try_wait().map_err(|error| {
                MacosUpdateLaunchError::new(format!(
                    "Could not inspect detached macOS update helper: {error}"
                ))
            })? {
                return Err(MacosUpdateLaunchError::new(format!(
                    "Detached macOS update helper closed readiness before success with status {status}."
                )));
            }
            return Err(MacosUpdateLaunchError::new(
                "Detached macOS update helper closed readiness before signaling success.",
            ));
        }
        if let Some(status) = child.try_wait().map_err(|error| {
            MacosUpdateLaunchError::new(format!(
                "Could not inspect detached macOS update helper: {error}"
            ))
        })? {
            return Err(MacosUpdateLaunchError::new(format!(
                "Detached macOS update helper exited before readiness with status {status}."
            )));
        }
        if Instant::now() >= deadline {
            stop_child(child);
            return Err(MacosUpdateLaunchError::new(
                "Timed out waiting for the macOS update helper readiness handshake.",
            ));
        }
    }
}

/// Launch a detached, signed copy of the embedded macOS update helper and wait until it has completed all
/// network/trust/staging work and signaled the process-handoff readiness boundary.
///
/// `smu-app` composes this primitive behind its guarded apply/restart path and requests app shutdown only after
/// this function succeeds. Production automatic apply remains disabled by the app-side updater capability gate.
///
/// # Errors
///
/// Returns before caller shutdown if the launch target, embedded helper, detached copy, signer, process spawn, or
/// readiness handshake cannot be validated safely.
pub fn launch_macos_update_helper(
    target_version: &str,
) -> Result<MacosUpdateLaunch, MacosUpdateLaunchError> {
    if !is_canonical_manifest_version(target_version) {
        return Err(MacosUpdateLaunchError::new(
            "macOS updater refused a non-canonical target version.",
        ));
    }
    let installed_bundle = macos_update_bundle_if_replaceable()
        .map_err(|error| MacosUpdateLaunchError::new(error.to_string()))?;
    let installed_helper = installed_bundle.join("Contents/MacOS").join(HELPER_NAME);
    let old_pid = libc::pid_t::try_from(std::process::id()).map_err(|_| {
        MacosUpdateLaunchError::new("Running SMU process id did not fit the macOS helper contract.")
    })?;
    let mut launch_directory = DetachedLaunchDirectory::create(old_pid)?;
    let (detached_helper, _helper_guard) =
        stage_detached_helper(&installed_bundle, &installed_helper, &launch_directory)?;
    require_directory_path_identity(&launch_directory.path, &launch_directory.guard)?;
    if path_identity(&detached_helper)? != file_identity(&_helper_guard)? {
        return Err(MacosUpdateLaunchError::new(
            "Detached macOS update helper changed identity before process launch.",
        ));
    }
    let current_dir = installed_bundle.parent().ok_or_else(|| {
        MacosUpdateLaunchError::new(
            "Could not resolve the installed macOS app containing directory.",
        )
    })?;
    let (mut readiness_read, readiness_write) = create_readiness_pipe()?;
    let readiness_fd = readiness_write.as_raw_fd();
    let mut child = Command::new(&detached_helper)
        .arg("--apply-update")
        .arg(target_version)
        .arg(&installed_bundle)
        .arg(old_pid.to_string())
        .arg(readiness_fd.to_string())
        .current_dir(current_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            MacosUpdateLaunchError::new(format!(
                "Could not launch detached macOS update helper: {error}"
            ))
        })?;
    drop(readiness_write);
    if let Err(error) = wait_for_ready(&mut child, &mut readiness_read) {
        stop_child(&mut child);
        return Err(error);
    }
    launch_directory.retain();
    Ok(MacosUpdateLaunch { helper_pid: child.id() })
}
