use smu_updater::{
    CurlUpdateTransport, UpdateChecker, UpdatePlatform, activate_prepared_macos_replacement,
    compare_versions, is_canonical_manifest_version, is_updater_version_allowed,
    macos_update_bundle_if_replaceable_at, prepare_macos_replacement_for_bundle,
    require_matching_macos_release_signer, select_update_asset,
    stage_verified_macos_update_asset_for_bundle,
};
use std::cmp::Ordering;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write as _;
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
use std::os::unix::ffi::OsStringExt as _;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{SystemTime, UNIX_EPOCH};

const APPLY_UPDATE_FLAG: &str = "--apply-update";
const APP_VERSION_TEXT: &str = include_str!("../../../../version");
const PRODUCT_EXECUTABLE: &str = "Spencer-Macro-Utilities";
const OLD_PROCESS_WAIT_POLLS: usize = 150;
const OLD_PROCESS_POLL_NANOS: libc::c_long = 100_000_000;
const TEMP_ATTEMPTS: usize = 128;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
struct ApplyUpdateArgs {
    target_version: String,
    installed_bundle: PathBuf,
    old_pid: libc::pid_t,
    ready_path: PathBuf,
}

fn helper_version() -> &'static str {
    APP_VERSION_TEXT.trim()
}

fn parse_apply_update_args<I>(args: I) -> Result<ApplyUpdateArgs, String>
where
    I: IntoIterator<Item = OsString>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    if args.get(1).and_then(|value| value.to_str()) != Some(APPLY_UPDATE_FLAG) || args.len() != 6 {
        return Err("Rust macOS update helper received invalid arguments.".to_owned());
    }
    let target_version = args[2]
        .to_str()
        .ok_or_else(|| "Rust macOS update helper received a non-UTF-8 target version.".to_owned())?
        .to_owned();
    if !is_canonical_manifest_version(&target_version) {
        return Err("Rust macOS update helper refused a non-canonical target version.".to_owned());
    }
    let installed_bundle = PathBuf::from(&args[3]);
    let old_pid = args[4]
        .to_str()
        .ok_or_else(|| "Rust macOS update helper received a non-UTF-8 process id.".to_owned())?
        .parse::<libc::pid_t>()
        .map_err(|_| "Rust macOS update helper received an invalid process id.".to_owned())?;
    let own_pid = libc::pid_t::try_from(std::process::id()).unwrap_or(libc::pid_t::MAX);
    if old_pid <= 0 || old_pid == own_pid {
        return Err("Rust macOS update helper received an invalid process id.".to_owned());
    }
    let ready_path = PathBuf::from(&args[5]);
    Ok(ApplyUpdateArgs { target_version, installed_bundle, old_pid, ready_path })
}

fn canonical_regular_file(path: &Path, label: &str) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Could not inspect {label}: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("Rust macOS update helper refused a non-regular {label}."));
    }
    fs::canonicalize(path).map_err(|error| format!("Could not resolve {label}: {error}"))
}

fn installed_executable(installed_bundle: &Path) -> Result<PathBuf, String> {
    canonical_regular_file(
        &installed_bundle.join("Contents/MacOS").join(PRODUCT_EXECUTABLE),
        "installed SMU executable",
    )
}

fn path_has_extended_acl(path: &Path) -> Result<bool, String> {
    let output = Command::new("/bin/ls")
        .env("LC_ALL", "C")
        .args(["-l", "-d", "-e"])
        .arg(path)
        .output()
        .map_err(|error| format!("Could not inspect detached macOS helper ACL: {error}"))?;
    if !output.status.success() {
        return Err("Could not inspect detached macOS helper ACL.".to_owned());
    }
    let listing = String::from_utf8_lossy(&output.stdout);
    Ok(listing.lines().skip(1).any(|line| {
        line.trim_start().split_once(':').is_some_and(|(index, _)| {
            !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())
        })
    }))
}

fn detached_current_helper(installed_bundle: &Path) -> Result<PathBuf, String> {
    let helper = canonical_regular_file(
        &std::env::current_exe()
            .map_err(|error| format!("Could not resolve macOS update helper: {error}"))?,
        "macOS update helper",
    )?;
    if helper.starts_with(installed_bundle) {
        return Err(
            "Rust macOS update helper must run from a detached path outside the installed app bundle."
                .to_owned(),
        );
    }
    let helper_parent = helper.parent().ok_or_else(|| {
        "Rust macOS update helper could not resolve its detached directory.".to_owned()
    })?;
    let parent_metadata = fs::symlink_metadata(helper_parent)
        .map_err(|error| format!("Could not inspect detached macOS helper directory: {error}"))?;
    if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
        return Err("Rust macOS update helper refused a non-directory detached parent.".to_owned());
    }
    if parent_metadata.uid() != unsafe { libc::geteuid() } || parent_metadata.mode() & 0o077 != 0 {
        return Err(
            "Rust macOS update helper requires a private detached directory owned by the current user."
                .to_owned(),
        );
    }
    if path_has_extended_acl(helper_parent)? {
        return Err("Rust macOS update helper refuses a detached directory with an extended ACL."
            .to_owned());
    }
    require_matching_macos_release_signer(installed_bundle, &helper)
        .map_err(|error| error.to_string())?;
    Ok(helper)
}

#[derive(Debug)]
struct ReadyMarker(PathBuf);

impl ReadyMarker {
    fn create(helper: &Path, ready_path: &Path) -> Result<Self, String> {
        let helper_parent = helper.parent().ok_or_else(|| {
            "Rust macOS update helper could not resolve its detached directory.".to_owned()
        })?;
        if ready_path.parent() != Some(helper_parent)
            || ready_path.file_name() != Some(OsStr::new("ready"))
        {
            return Err(
                "Rust macOS update helper refused a readiness marker outside its detached directory."
                    .to_owned(),
            );
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(ready_path)
            .map_err(|error| format!("Could not create macOS updater readiness marker: {error}"))?;
        file.write_all(std::process::id().to_string().as_bytes())
            .map_err(|error| format!("Could not write macOS updater readiness marker: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("Could not sync macOS updater readiness marker: {error}"))?;
        Ok(Self(ready_path.to_path_buf()))
    }
}

impl Drop for ReadyMarker {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn process_executable_path(pid: libc::pid_t) -> Result<PathBuf, String> {
    let capacity = usize::try_from(libc::PROC_PIDPATHINFO_MAXSIZE)
        .map_err(|_| "macOS process-path capacity was invalid.".to_owned())?;
    let mut buffer = vec![0_u8; capacity];
    let buffer_size = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
    // SAFETY: buffer is writable for buffer_size bytes and pid is supplied to the documented libproc API.
    let length = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer_size) };
    if length <= 0 {
        return Err(format!(
            "Could not resolve executable path for macOS process {pid}: {}",
            std::io::Error::last_os_error()
        ));
    }
    let length = usize::try_from(length)
        .map_err(|_| "macOS process executable path length was invalid.".to_owned())?;
    if length > buffer.len() {
        return Err("macOS process executable path exceeded its buffer.".to_owned());
    }
    let path_length = buffer[..length].iter().position(|byte| *byte == 0).unwrap_or(length);
    if path_length == 0 {
        return Err("macOS process executable path was empty.".to_owned());
    }
    let path = PathBuf::from(OsString::from_vec(buffer[..path_length].to_vec()));
    fs::canonicalize(path).map_err(|error| {
        format!("Could not canonicalize executable for macOS process {pid}: {error}")
    })
}

fn old_process_is_gone_or_reused(pid: libc::pid_t, expected_executable: &Path) -> bool {
    match process_executable_path(pid) {
        Ok(actual_executable) => actual_executable != expected_executable,
        Err(_) => {
            // SAFETY: signal 0 performs existence/permission checking without delivering a signal.
            let result = unsafe { libc::kill(pid, 0) };
            if result == 0 {
                return false;
            }
            std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        }
    }
}

#[derive(Debug)]
struct PinnedOldProcess {
    queue: OwnedFd,
    pid: libc::pid_t,
}

impl PinnedOldProcess {
    fn register(pid: libc::pid_t, expected_executable: &Path) -> Result<Self, String> {
        // SAFETY: kqueue takes no arguments and returns a new file descriptor on success.
        let queue_fd = unsafe { libc::kqueue() };
        if queue_fd < 0 {
            return Err(format!(
                "Could not create macOS update process monitor: {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: queue_fd is newly owned after successful kqueue().
        let queue = unsafe { OwnedFd::from_raw_fd(queue_fd) };
        let ident = usize::try_from(pid)
            .map_err(|_| "macOS update process id did not fit kqueue identity.".to_owned())?;
        let change = libc::kevent {
            ident,
            filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_ONESHOT,
            fflags: libc::NOTE_EXIT,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        // SAFETY: queue is live and change points to one initialized kevent; no event output is requested.
        let registered = unsafe {
            libc::kevent(
                queue.as_raw_fd(),
                &raw const change,
                1,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        };
        if registered != 0 {
            return Err(format!(
                "Could not register the old SMU process with kqueue: {}",
                std::io::Error::last_os_error()
            ));
        }
        let actual_executable = process_executable_path(pid)?;
        if actual_executable != expected_executable {
            return Err(
                "Rust macOS update helper refused an old process whose executable did not match the installed app."
                    .to_owned(),
            );
        }
        Ok(Self { queue, pid })
    }

    fn wait_for_exit(&self) -> Result<(), String> {
        for _ in 0..OLD_PROCESS_WAIT_POLLS {
            let timeout = libc::timespec { tv_sec: 0, tv_nsec: OLD_PROCESS_POLL_NANOS };
            let mut event = std::mem::MaybeUninit::<libc::kevent>::zeroed();
            // SAFETY: queue remains live, event points to writable storage, and timeout is initialized.
            let result = unsafe {
                libc::kevent(
                    self.queue.as_raw_fd(),
                    std::ptr::null(),
                    0,
                    event.as_mut_ptr(),
                    1,
                    &raw const timeout,
                )
            };
            if result == 0 {
                continue;
            }
            if result < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(format!("Failed while waiting for old SMU process: {error}"));
            }
            // SAFETY: kevent returned one event and initialized the output structure. Apple's kevent is packed,
            // so copy fields into aligned locals before any formatting operation could borrow them.
            let event = unsafe { event.assume_init() };
            let event_flags = event.flags;
            let event_data = event.data;
            let event_filter = event.filter;
            let event_ident = event.ident;
            let event_fflags = event.fflags;
            if event_flags & libc::EV_ERROR != 0 && event_data != 0 {
                return Err(format!(
                    "macOS process monitor reported error {event_data} for PID {}.",
                    self.pid
                ));
            }
            if event_filter == libc::EVFILT_PROC
                && event_ident == usize::try_from(self.pid).unwrap_or(usize::MAX)
                && event_fflags & libc::NOTE_EXIT != 0
            {
                return Ok(());
            }
            return Err("macOS process monitor returned an unexpected event.".to_owned());
        }
        Err("Timed out waiting for old SMU process to exit.".to_owned())
    }
}

#[derive(Debug)]
struct DownloadWorkspace {
    path: PathBuf,
    package: Option<PathBuf>,
}

impl DownloadWorkspace {
    fn create() -> Result<Self, String> {
        let base = std::env::temp_dir();
        let epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        for _ in 0..TEMP_ATTEMPTS {
            let counter = TEMP_COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
            let path = base.join(format!(
                "smu-macos-helper-download-{}-{epoch}-{counter}",
                std::process::id()
            ));
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(&path) {
                Ok(()) => return Ok(Self { path, package: None }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(format!(
                        "Could not create private macOS helper download directory: {error}"
                    ));
                }
            }
        }
        Err("Could not allocate a private macOS helper download directory.".to_owned())
    }

    fn package_path(&mut self, asset_name: &str) -> Result<PathBuf, String> {
        if !is_safe_asset_leaf(asset_name) {
            return Err(
                "Rust macOS update helper refused an unsafe release asset filename.".to_owned()
            );
        }
        let package = self.path.join(asset_name);
        self.package = Some(package.clone());
        Ok(package)
    }
}

impl Drop for DownloadWorkspace {
    fn drop(&mut self) {
        if let Some(package) = &self.package {
            let _ = fs::remove_file(package);
        }
        let _ = fs::remove_dir(&self.path);
    }
}

fn is_safe_asset_leaf(name: &str) -> bool {
    if name.is_empty() || name.chars().any(|character| matches!(character, '\\' | '\0' | ':')) {
        return false;
    }
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn bundle_version(bundle: &Path) -> Result<String, String> {
    let plist = bundle.join("Contents/Info.plist");
    let output = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print :CFBundleShortVersionString"])
        .arg(&plist)
        .output()
        .map_err(|error| format!("Could not launch PlistBuddy for {}: {error}", plist.display()))?;
    if !output.status.success() {
        return Err(format!("Could not read CFBundleShortVersionString from {}.", plist.display()));
    }
    let version = String::from_utf8(output.stdout)
        .map_err(|_| "macOS app version was not valid UTF-8.".to_owned())?
        .trim()
        .to_owned();
    if !is_canonical_manifest_version(&version) {
        return Err("macOS app bundle contained a non-canonical version.".to_owned());
    }
    Ok(version)
}

fn mark_permissions_for_repair() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let support = PathBuf::from(home).join("Library/Application Support/Spencer Macro Utilities");
    if fs::create_dir_all(&support).is_ok() {
        let _ = fs::write(support.join("macos-permissions-may-need-repair"), b"");
    }
}

fn clear_quarantine(bundle: &Path) {
    let _ = Command::new("/usr/bin/xattr")
        .args(["-d", "-r", "com.apple.quarantine"])
        .arg(bundle)
        .status();
}

fn launch_bundle(bundle: &Path) -> Result<(), String> {
    let status = Command::new("/usr/bin/open")
        .arg("-n")
        .arg(bundle)
        .status()
        .map_err(|error| format!("Could not launch updated SMU app: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Updated SMU app could not be opened; open exited with {status}."))
    }
}

fn fetch_and_stage_update(
    target_version: &str,
    installed_bundle: &Path,
) -> Result<(DownloadWorkspace, smu_updater::StagedMacosBundle), String> {
    let mut checker =
        UpdateChecker::new(CurlUpdateTransport::macos(), UpdatePlatform::MacOs, false);
    let release =
        checker.fetch_release_version(target_version).map_err(|error| error.to_string())?;
    if !is_updater_version_allowed(&release, helper_version()) {
        return Err("This SMU macOS update helper is too old for the requested release.".to_owned());
    }
    let asset = select_update_asset(&release, UpdatePlatform::MacOs)
        .ok_or_else(|| "Requested release did not include a macOS update package.".to_owned())?;
    let mut workspace = DownloadWorkspace::create()?;
    let package = workspace.package_path(&asset.name)?;
    checker
        .into_transport()
        .download_to_file(&asset.download_url, &package)
        .map_err(|error| error.to_string())?;
    let staged = stage_verified_macos_update_asset_for_bundle(&asset, &package, installed_bundle)
        .map_err(|error| error.to_string())?;
    Ok((workspace, staged))
}

fn recover_activation_failure(
    error: smu_updater::MacosReplaceError,
    installed_bundle: &Path,
) -> Result<(), String> {
    let message = error.to_string();
    if !error.recovery_available() {
        let _ = launch_bundle(installed_bundle);
        return Err(format!(
            "Updated SMU app could not be activated: {message}. The unchanged installed app was relaunched."
        ));
    }
    match error.recover() {
        Ok(()) => {
            let _ = launch_bundle(installed_bundle);
            Err(format!(
                "Updated SMU app could not be activated: {message}. The previous app was restored and a relaunch was attempted."
            ))
        }
        Err(error) => Err(format!(
            "Updated SMU app could not be activated: {message}. Automatic recovery could not be completed safely: {error}"
        )),
    }
}

fn finish_commit(
    transaction: smu_updater::MacosReplacementTransaction,
    installed_bundle: &Path,
) -> Result<(), String> {
    match transaction.commit() {
        Ok(()) => Ok(()),
        Err(error) if error.recovery_available() => {
            let message = error.to_string();
            match error.recover() {
                Ok(()) => {
                    let _ = launch_bundle(installed_bundle);
                    Err(format!(
                        "Updated SMU app could not be committed safely: {message}. The previous app was restored and a relaunch was attempted."
                    ))
                }
                Err(error) => Err(format!(
                    "Updated SMU app could not be committed safely: {message}. Automatic recovery could not be completed: {error}"
                )),
            }
        }
        Err(error) => Err(format!(
            "Updated SMU app was launched, but rollback cleanup could not be completed: {error}"
        )),
    }
}

fn rollback_after_launch_failure(
    transaction: smu_updater::MacosReplacementTransaction,
    installed_bundle: &Path,
    launch_error: &str,
) -> Result<(), String> {
    match transaction.rollback() {
        Ok(()) => {
            let _ = launch_bundle(installed_bundle);
            Err(format!(
                "{launch_error} The previous SMU app was restored and a relaunch was attempted."
            ))
        }
        Err(error) => {
            let recovery_available = error.recovery_available();
            let recovery = if recovery_available { error.recover() } else { Err(error) };
            match recovery {
                Ok(()) => {
                    let _ = launch_bundle(installed_bundle);
                    Err(format!(
                        "{launch_error} The previous SMU app was restored after a recovery retry."
                    ))
                }
                Err(error) => Err(format!(
                    "{launch_error} Automatic rollback could not be completed safely: {error}"
                )),
            }
        }
    }
}

pub fn run() -> Result<(), String> {
    let args = parse_apply_update_args(std::env::args_os())?;
    let installed_bundle = macos_update_bundle_if_replaceable_at(&args.installed_bundle)
        .map_err(|error| error.to_string())?;
    let helper = detached_current_helper(&installed_bundle)?;
    let installed_executable = installed_executable(&installed_bundle)?;
    let old_process = PinnedOldProcess::register(args.old_pid, &installed_executable)?;

    let installed_version = bundle_version(&installed_bundle)?;
    if compare_versions(&args.target_version, &installed_version) != Ordering::Greater {
        return Err("Rust macOS update helper refused a non-upgrade target version.".to_owned());
    }

    let (_workspace, staged) = fetch_and_stage_update(&args.target_version, &installed_bundle)?;
    let staged_version = bundle_version(staged.app_path())?;
    if staged_version != args.target_version {
        return Err("Downloaded macOS app version did not match the requested release.".to_owned());
    }
    let prepared = prepare_macos_replacement_for_bundle(&staged, &installed_bundle)
        .map_err(|error| error.to_string())?;

    // The parent must not exit until every network/trust/staging check has completed. The readiness marker is
    // therefore the final handoff signal immediately before waiting on the already-registered process object.
    let _ready = ReadyMarker::create(&helper, &args.ready_path)?;
    if let Err(error) = old_process.wait_for_exit() {
        if old_process_is_gone_or_reused(args.old_pid, &installed_executable) {
            let _ = launch_bundle(&installed_bundle);
        }
        return Err(error);
    }
    let transaction = match activate_prepared_macos_replacement(prepared) {
        Ok(transaction) => transaction,
        Err(error) => return recover_activation_failure(error, &installed_bundle),
    };
    mark_permissions_for_repair();
    clear_quarantine(&installed_bundle);
    if let Err(error) = launch_bundle(&installed_bundle) {
        return rollback_after_launch_failure(transaction, &installed_bundle, &error);
    }
    finish_commit(transaction, &installed_bundle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_require_canonical_version_bundle_and_distinct_positive_pid() {
        let parsed = parse_apply_update_args([
            OsString::from("helper"),
            OsString::from(APPLY_UPDATE_FLAG),
            OsString::from("3.4.0"),
            OsString::from("/Applications/Spencer Macro Utilities.app"),
            OsString::from("4242"),
            OsString::from("/tmp/smu-helper/ready"),
        ]);
        assert!(parsed.is_ok());
        assert!(
            parse_apply_update_args([
                OsString::from("helper"),
                OsString::from(APPLY_UPDATE_FLAG),
                OsString::from("v3.4.0"),
                OsString::from("/Applications/SMU.app"),
                OsString::from("4242"),
                OsString::from("/tmp/smu-helper/ready"),
            ])
            .is_err()
        );
        assert!(
            parse_apply_update_args([
                OsString::from("helper"),
                OsString::from(APPLY_UPDATE_FLAG),
                OsString::from("3.4.0"),
                OsString::from("/Applications/SMU.app"),
                OsString::from("0"),
                OsString::from("/tmp/smu-helper/ready"),
            ])
            .is_err()
        );
    }

    #[test]
    fn release_asset_name_must_be_one_safe_leaf() {
        assert!(is_safe_asset_leaf("Spencer-Macro-Utilities-V3.4.0-macOS-universal.zip"));
        for unsafe_name in
            ["", "../update.zip", "dir/update.zip", "dir\\update.zip", "C:update.zip"]
        {
            assert!(!is_safe_asset_leaf(unsafe_name));
        }
    }
}
