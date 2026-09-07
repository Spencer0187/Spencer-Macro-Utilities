use crate::{
    MAXIMUM_UPDATE_BYTES, UpdateChecker, UpdatePlatform, UpdateTransport, WinInetUpdateTransport,
    is_canonical_manifest_version, is_updater_version_allowed,
    require_matching_authenticode_signer, select_update_asset, verify_downloaded_asset_bytes,
    windows_file_version,
};
use sha2::{Digest as _, Sha256};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_READ,
    },
    System::Threading::{CreateEventW, WaitForSingleObject},
};

const HELPER_EXE: &str = "SMU-Update-Helper.exe";
const TEMP_ATTEMPTS: usize = 128;
const MAXIMUM_HELPER_BYTES: u64 = 64 * 1024 * 1024;
const READINESS_TIMEOUT: Duration = Duration::from_secs(180);
const READINESS_POLL: Duration = Duration::from_millis(50);
const READINESS_SURVIVAL: Duration = Duration::from_millis(100);
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsUpdateLaunchError(String);

impl WindowsUpdateLaunchError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for WindowsUpdateLaunchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for WindowsUpdateLaunchError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowsUpdateLaunch {
    pub helper_pid: u32,
}

#[derive(Debug)]
struct ReadinessEvent(HANDLE);

impl ReadinessEvent {
    fn create() -> Result<Self, WindowsUpdateLaunchError> {
        // SAFETY: null security/name pointers create a private unnamed, manual-reset, initially unsignaled event.
        let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if handle.is_null() {
            return Err(WindowsUpdateLaunchError::new(format!(
                "Could not create the Windows helper readiness event: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(Self(handle))
    }

    const fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for ReadinessEvent {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns the event handle created above.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn wait_for_helper_readiness(
    event: &ReadinessEvent,
    child: &mut Child,
) -> Result<(), WindowsUpdateLaunchError> {
    let deadline = Instant::now() + READINESS_TIMEOUT;
    let poll_ms = u32::try_from(READINESS_POLL.as_millis()).unwrap_or(u32::MAX);
    loop {
        // SAFETY: event remains live and this is a bounded wait.
        match unsafe { WaitForSingleObject(event.raw(), poll_ms) } {
            WAIT_OBJECT_0 => {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        return Err(WindowsUpdateLaunchError::new(format!(
                            "Windows update helper exited after signaling readiness with status {status}."
                        )));
                    }
                    Ok(None) => {}
                    Err(error) => {
                        stop_child(child);
                        return Err(WindowsUpdateLaunchError::new(format!(
                            "Could not inspect the Windows update helper after readiness: {error}"
                        )));
                    }
                }
                std::thread::sleep(READINESS_SURVIVAL);
                match child.try_wait() {
                    Ok(Some(status)) => {
                        return Err(WindowsUpdateLaunchError::new(format!(
                            "Windows update helper did not survive the handoff window: {status}."
                        )));
                    }
                    Ok(None) => {}
                    Err(error) => {
                        stop_child(child);
                        return Err(WindowsUpdateLaunchError::new(format!(
                            "Could not inspect the Windows update helper after the handoff window: {error}"
                        )));
                    }
                }
                return Ok(());
            }
            WAIT_TIMEOUT => {}
            WAIT_FAILED => {
                let error = std::io::Error::last_os_error();
                stop_child(child);
                return Err(WindowsUpdateLaunchError::new(format!(
                    "Could not wait for the Windows update helper readiness handshake: {error}"
                )));
            }
            other => {
                stop_child(child);
                return Err(WindowsUpdateLaunchError::new(format!(
                    "Unexpected Windows update helper readiness wait result: {other}."
                )));
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                return Err(WindowsUpdateLaunchError::new(format!(
                    "Windows update helper exited before signaling readiness with status {status}."
                )));
            }
            Ok(None) => {}
            Err(error) => {
                stop_child(child);
                return Err(WindowsUpdateLaunchError::new(format!(
                    "Could not inspect the Windows update helper before readiness: {error}"
                )));
            }
        }
        if Instant::now() >= deadline {
            stop_child(child);
            return Err(WindowsUpdateLaunchError::new(
                "Timed out waiting for the Windows update helper readiness handshake.",
            ));
        }
    }
}

#[derive(Debug)]
struct DetachedLaunchDirectory {
    path: PathBuf,
    guard: Option<File>,
    retain: bool,
}

impl DetachedLaunchDirectory {
    fn create() -> Result<Self, WindowsUpdateLaunchError> {
        let base = std::env::temp_dir();
        let epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        for _ in 0..TEMP_ATTEMPTS {
            let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path =
                base.join(format!("smu-update-launch-{}-{epoch}-{counter}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => match open_pinned_directory(&path) {
                    Ok(guard) => return Ok(Self { path, guard: Some(guard), retain: false }),
                    Err(error) => {
                        let _ = fs::remove_dir(&path);
                        return Err(error);
                    }
                },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(WindowsUpdateLaunchError::new(format!(
                        "Could not create the detached Windows update directory: {error}"
                    )));
                }
            }
        }
        Err(WindowsUpdateLaunchError::new(
            "Could not allocate a detached Windows update directory.",
        ))
    }

    fn retain(&mut self) {
        self.retain = true;
    }
}

impl Drop for DetachedLaunchDirectory {
    fn drop(&mut self) {
        self.guard = None;
        if !self.retain {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn is_safe_windows_leaf_name(name: &str) -> bool {
    !name.is_empty()
        && !matches!(name, "." | "..")
        && !name.ends_with([' ', '.'])
        && !name.bytes().any(|byte| {
            byte < 0x20
                || matches!(byte, b'<' | b'>' | b':' | b'"' | b'/' | b'\\' | b'|' | b'?' | b'*')
        })
}

fn canonical_file(path: &Path, label: &str) -> Result<PathBuf, WindowsUpdateLaunchError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        WindowsUpdateLaunchError::new(format!("Could not inspect {label}: {error}"))
    })?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(WindowsUpdateLaunchError::new(format!(
            "Windows updater refused a non-regular or reparse-point {label}."
        )));
    }
    fs::canonicalize(path).map_err(|error| {
        WindowsUpdateLaunchError::new(format!("Could not resolve {label}: {error}"))
    })
}

fn open_pinned_directory(path: &Path) -> Result<File, WindowsUpdateLaunchError> {
    let mut options = OpenOptions::new();
    options
        .access_mode(0)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    let file = options.open(path).map_err(|error| {
        WindowsUpdateLaunchError::new(format!(
            "Could not pin the detached Windows update directory: {error}"
        ))
    })?;
    let metadata = file.metadata().map_err(|error| {
        WindowsUpdateLaunchError::new(format!(
            "Could not inspect the detached Windows update directory: {error}"
        ))
    })?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(WindowsUpdateLaunchError::new(
            "Windows updater refused a reparse-point detached update directory.",
        ));
    }
    Ok(file)
}

fn open_pinned_regular_file(path: &Path, label: &str) -> Result<File, WindowsUpdateLaunchError> {
    let mut options = OpenOptions::new();
    options.read(true).share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    let file = options.open(path).map_err(|error| {
        WindowsUpdateLaunchError::new(format!("Could not open and pin {label}: {error}"))
    })?;
    let metadata = file.metadata().map_err(|error| {
        WindowsUpdateLaunchError::new(format!("Could not inspect pinned {label}: {error}"))
    })?;
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.len() == 0
        || metadata.len() > MAXIMUM_HELPER_BYTES
    {
        return Err(WindowsUpdateLaunchError::new(format!(
            "Windows updater refused an invalid or oversized {label}."
        )));
    }
    Ok(file)
}

fn read_pinned_file(file: &mut File, label: &str) -> Result<Vec<u8>, WindowsUpdateLaunchError> {
    file.seek(SeekFrom::Start(0)).map_err(|error| {
        WindowsUpdateLaunchError::new(format!("Could not rewind {label}: {error}"))
    })?;
    let size = file
        .metadata()
        .map_err(|error| {
            WindowsUpdateLaunchError::new(format!("Could not inspect {label}: {error}"))
        })?
        .len();
    if size == 0 || size > MAXIMUM_HELPER_BYTES {
        return Err(WindowsUpdateLaunchError::new(format!(
            "Windows updater refused an invalid or oversized {label}."
        )));
    }
    let capacity = usize::try_from(size)
        .map_err(|_| WindowsUpdateLaunchError::new(format!("{label} was too large to copy.")))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(MAXIMUM_HELPER_BYTES.saturating_add(1)).read_to_end(&mut bytes).map_err(|error| {
        WindowsUpdateLaunchError::new(format!("Could not read {label}: {error}"))
    })?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != size {
        return Err(WindowsUpdateLaunchError::new(format!(
            "{label} changed while it was being copied."
        )));
    }
    Ok(bytes)
}

fn write_new_file(path: &Path, bytes: &[u8], label: &str) -> Result<(), WindowsUpdateLaunchError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).share_mode(FILE_SHARE_READ);
    let mut file = options.open(path).map_err(|error| {
        WindowsUpdateLaunchError::new(format!("Could not create {label}: {error}"))
    })?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| WindowsUpdateLaunchError::new(format!("Could not write {label}: {error}")))
}

fn stage_detached_helper(
    current_app: &Path,
    installed_helper: &Path,
    directory: &Path,
) -> Result<(PathBuf, File), WindowsUpdateLaunchError> {
    let mut installed_handle =
        open_pinned_regular_file(installed_helper, "the installed update helper")?;
    require_matching_authenticode_signer(current_app, installed_helper)
        .map_err(|error| WindowsUpdateLaunchError::new(error.to_string()))?;
    if windows_file_version(current_app)
        .map_err(|error| WindowsUpdateLaunchError::new(error.to_string()))?
        != windows_file_version(installed_helper)
            .map_err(|error| WindowsUpdateLaunchError::new(error.to_string()))?
    {
        return Err(WindowsUpdateLaunchError::new(
            "Installed update helper version did not match the running SMU executable.",
        ));
    }
    let helper_bytes = read_pinned_file(&mut installed_handle, "the installed update helper")?;
    let helper_digest: [u8; 32] = Sha256::digest(&helper_bytes).into();
    let detached = directory.join(HELPER_EXE);
    write_new_file(&detached, &helper_bytes, "the detached update helper")?;
    let mut detached_handle = open_pinned_regular_file(&detached, "the detached update helper")?;
    let detached_bytes = read_pinned_file(&mut detached_handle, "the detached update helper")?;
    let detached_digest: [u8; 32] = Sha256::digest(&detached_bytes).into();
    if detached_digest != helper_digest {
        return Err(WindowsUpdateLaunchError::new(
            "Detached update helper did not match the installed signed helper.",
        ));
    }
    require_matching_authenticode_signer(current_app, &detached)
        .map_err(|error| WindowsUpdateLaunchError::new(error.to_string()))?;
    if windows_file_version(installed_helper)
        .map_err(|error| WindowsUpdateLaunchError::new(error.to_string()))?
        != windows_file_version(&detached)
            .map_err(|error| WindowsUpdateLaunchError::new(error.to_string()))?
    {
        return Err(WindowsUpdateLaunchError::new(
            "Detached update helper version did not match the installed helper.",
        ));
    }
    Ok((detached, detached_handle))
}

/// Download the exact official Windows package and launch a detached, pinned copy of the installed
/// signed update helper. The app composes this primitive behind an updater capability that remains false until
/// production bundle-wide signing/trust is deliberately promoted; the parent/helper readiness handoff is already
/// implemented by duplicating a private unnamed event from the validated parent process.
///
/// # Errors
///
/// Returns an error before application shutdown if the release contract, package bytes, helper trust,
/// helper copy, or process launch cannot be validated safely.
pub fn launch_windows_update_helper(
    target_version: &str,
) -> Result<WindowsUpdateLaunch, WindowsUpdateLaunchError> {
    if !is_canonical_manifest_version(target_version) {
        return Err(WindowsUpdateLaunchError::new(
            "Windows updater refused a non-canonical target version.",
        ));
    }
    let current_app = canonical_file(
        &std::env::current_exe().map_err(|error| {
            WindowsUpdateLaunchError::new(format!(
                "Could not resolve the running SMU executable: {error}"
            ))
        })?,
        "running SMU executable",
    )?;
    let install_root = current_app.parent().ok_or_else(|| {
        WindowsUpdateLaunchError::new("Could not resolve the SMU installation directory.")
    })?;
    let installed_helper =
        canonical_file(&install_root.join(HELPER_EXE), "installed update helper")?;
    let current_version = windows_file_version(&current_app)
        .map_err(|error| WindowsUpdateLaunchError::new(error.to_string()))?;
    if current_version.revision != 0 {
        return Err(WindowsUpdateLaunchError::new(
            "Running SMU executable used an unsupported four-part FILEVERSION.",
        ));
    }
    let local_version =
        format!("{}.{}.{}", current_version.major, current_version.minor, current_version.patch);

    let mut checker = UpdateChecker::new(WinInetUpdateTransport, UpdatePlatform::Windows, false);
    let release = checker
        .fetch_release_version(target_version)
        .map_err(|error| WindowsUpdateLaunchError::new(error.to_string()))?;
    if !is_updater_version_allowed(&release, &local_version) {
        return Err(WindowsUpdateLaunchError::new(
            "Updater refused to launch for a release requiring a newer updater.",
        ));
    }
    let asset = select_update_asset(&release, UpdatePlatform::Windows).ok_or_else(|| {
        WindowsUpdateLaunchError::new("Requested release did not include a Windows update package.")
    })?;
    if asset.size_bytes == 0 || asset.size_bytes > MAXIMUM_UPDATE_BYTES {
        return Err(WindowsUpdateLaunchError::new(
            "Updater refused an invalid or oversized Windows release asset.",
        ));
    }
    let mut transport = checker.into_transport();
    let package = transport
        .get(&asset.download_url)
        .map_err(|error| WindowsUpdateLaunchError::new(error.to_string()))?;
    verify_downloaded_asset_bytes(&asset, &package)
        .map_err(|error| WindowsUpdateLaunchError::new(error.to_string()))?;

    if !is_safe_windows_leaf_name(&asset.name) {
        return Err(WindowsUpdateLaunchError::new(
            "Windows updater refused an unsafe release asset filename.",
        ));
    }

    let mut launch_directory = DetachedLaunchDirectory::create()?;
    let package_path = launch_directory.path.join(&asset.name);
    write_new_file(&package_path, &package, "the downloaded Windows update package")?;
    let (detached_helper, _helper_guard) =
        stage_detached_helper(&current_app, &installed_helper, &launch_directory.path)?;

    let readiness = ReadinessEvent::create()?;
    let mut child = Command::new(&detached_helper)
        .arg("--apply-update")
        .arg(target_version)
        .arg(&package_path)
        .arg(install_root)
        .arg(std::process::id().to_string())
        .arg((readiness.raw() as usize).to_string())
        .current_dir(install_root)
        .spawn()
        .map_err(|error| {
            WindowsUpdateLaunchError::new(format!(
                "Could not launch the detached Windows update helper: {error}"
            ))
        })?;
    wait_for_helper_readiness(&readiness, &mut child)?;
    launch_directory.retain();
    Ok(WindowsUpdateLaunch { helper_pid: child.id() })
}
