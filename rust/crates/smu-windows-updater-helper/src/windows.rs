use smu_updater::{
    UpdateChecker, UpdatePlatform, WINDOWS_RELEASE_SIGNER_FILES,
    WINDOWS_UNCHANGED_UPSTREAM_LIBRARY_FILES, WINDOWS_UPSTREAM_SIGNER_CONTINUITY_FILES,
    WinInetUpdateTransport, WindowsFileVersion, WindowsReplacementTransaction,
    extract_windows_bundle, is_canonical_manifest_version, is_updater_version_allowed,
    prepare_windows_bundle_tree, read_verified_update_asset, replace_windows_bundle_tree,
    require_matching_authenticode_signer, require_unchanged_windows_upstream_library,
    select_update_asset, windows_bundle_file_sha256, windows_file_version,
};
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::windows::fs::MetadataExt as _;
use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, ERROR_INVALID_PARAMETER, HANDLE,
        WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    Storage::FileSystem::{FILE_ATTRIBUTE_REPARSE_POINT, MOVEFILE_DELAY_UNTIL_REBOOT, MoveFileExW},
    System::{
        SystemInformation::GetSystemDirectoryW,
        Threading::{
            CREATE_NO_WINDOW, GetCurrentProcess, OpenProcess, PROCESS_DUP_HANDLE,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW,
            ResetEvent, SetEvent, WaitForSingleObject,
        },
    },
    UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
};

const APPLY_UPDATE_FLAG: &str = "--apply-update";
const APP_VERSION_TEXT: &str = include_str!("../../../../version");
const PRODUCT_EXE: &str = "Spencer-Macro-Utilities.exe";
const HELPER_EXE: &str = "SMU-Update-Helper.exe";
const OLD_PROCESS_WAIT: Duration = Duration::from_secs(20);
const RELAUNCH_SURVIVAL: Duration = Duration::from_secs(2);
const TEMP_ATTEMPTS: usize = 128;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
struct ProcessHandle(*mut core::ffi::c_void);

#[derive(Debug)]
struct ReadyEvent(HANDLE);

impl ReadyEvent {
    fn duplicate_from_parent(process: &ProcessHandle, value: usize) -> Result<Self, String> {
        if value == 0 || value == usize::MAX {
            return Err(
                "Rust Windows update helper received an invalid readiness handle.".to_owned()
            );
        }
        let source = value as HANDLE;
        let mut duplicate: HANDLE = std::ptr::null_mut();
        // SAFETY: process is the validated parent process handle with PROCESS_DUP_HANDLE; source is interpreted
        // in that process, and GetCurrentProcess returns the documented pseudo-handle for this helper process.
        if unsafe {
            DuplicateHandle(
                process.0,
                source,
                GetCurrentProcess(),
                &raw mut duplicate,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        } == 0
        {
            return Err(format!(
                "Rust Windows update helper could not duplicate its readiness event: {}",
                std::io::Error::last_os_error()
            ));
        }
        if duplicate.is_null() {
            return Err(
                "Rust Windows update helper received an empty duplicated readiness handle."
                    .to_owned(),
            );
        }
        // ResetEvent validates that the duplicated kernel object is an event and ensures it is unsignaled before
        // any network/staging work begins; the parent created this private event unsignaled.
        // SAFETY: duplicate is a newly owned handle returned by DuplicateHandle.
        if unsafe { ResetEvent(duplicate) } == 0 {
            let error = std::io::Error::last_os_error();
            // SAFETY: duplicate is newly owned by this helper on the success path above.
            unsafe {
                let _ = CloseHandle(duplicate);
            }
            return Err(format!(
                "Rust Windows update helper refused a non-event readiness handle: {error}"
            ));
        }
        Ok(Self(duplicate))
    }

    fn signal(&self) -> Result<(), String> {
        // SAFETY: the duplicated handle was validated as an event by ResetEvent.
        if unsafe { SetEvent(self.0) } == 0 {
            return Err(format!(
                "Rust Windows update helper could not signal readiness: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

impl Drop for ReadyEvent {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns the duplicated readiness event handle.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns a process handle returned by OpenProcess.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

pub fn show_error(message: &str) {
    let title = wide_null(OsStr::new("Spencer Macro Utilities Update Failed"));
    let body = wide_null(OsStr::new(message));
    // SAFETY: title and body are NUL-terminated UTF-16 strings and no owner window is required.
    unsafe {
        let _ =
            MessageBoxW(std::ptr::null_mut(), body.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR);
    }
}

fn schedule_delete_on_reboot(path: &Path) {
    let path = wide_null(path.as_os_str());
    // SAFETY: path is a NUL-terminated UTF-16 string and a null destination requests deletion.
    unsafe {
        let _ = MoveFileExW(path.as_ptr(), std::ptr::null(), MOVEFILE_DELAY_UNTIL_REBOOT);
    }
}

fn system_executable_path(name: &str, label: &str) -> Result<PathBuf, String> {
    let mut buffer = vec![0_u16; 32_768];
    // SAFETY: buffer is writable for its declared length and GetSystemDirectoryW writes UTF-16 data.
    let length = unsafe {
        GetSystemDirectoryW(buffer.as_mut_ptr(), u32::try_from(buffer.len()).unwrap_or(u32::MAX))
    };
    let length = usize::try_from(length)
        .map_err(|_| "Windows system directory path length was invalid.".to_owned())?;
    if length == 0 || length >= buffer.len() {
        return Err("Could not resolve the Windows system directory.".to_owned());
    }
    let mut path = PathBuf::from(OsString::from_wide(&buffer[..length]));
    path.push(name);
    canonical_file(&path, label)
}

fn spawn_cleanup_process(cleanup: &DetachedArtifactsCleanup) -> Result<(), String> {
    const CLEANUP_COMMAND: &str = concat!(
        "for /L %i in (1,1,120) do @( ",
        "if defined SMU_CLEANUP_PACKAGE del /F /Q \"%SMU_CLEANUP_PACKAGE%\" >NUL 2>&1 & ",
        "del /F /Q \"%SMU_CLEANUP_HELPER%\" >NUL 2>&1 & ",
        "rd /Q \"%SMU_CLEANUP_DIR%\" >NUL 2>&1 & ",
        "if not exist \"%SMU_CLEANUP_DIR%\" exit /B 0 & ",
        "\"%SMU_CLEANUP_DELAY%\" -n 2 127.0.0.1 >NUL )"
    );

    let command_processor =
        system_executable_path("cmd.exe", "the Windows system command processor")?;
    let cleanup_delay =
        system_executable_path("ping.exe", "the Windows system cleanup delay utility")?;
    let mut command = Command::new(command_processor);
    command
        .args(["/D", "/Q", "/E:ON", "/V:OFF", "/C", CLEANUP_COMMAND])
        .env("SMU_CLEANUP_HELPER", &cleanup.helper)
        .env("SMU_CLEANUP_DIR", &cleanup.directory)
        .env("SMU_CLEANUP_DELAY", cleanup_delay)
        .env("SMU_CLEANUP_PACKAGE", cleanup.package.as_deref().unwrap_or_else(|| Path::new("")))
        .creation_flags(CREATE_NO_WINDOW);
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not launch detached update cleanup: {error}"))
}

#[derive(Debug)]
struct DetachedArtifactsCleanup {
    helper: PathBuf,
    package: Option<PathBuf>,
    directory: PathBuf,
}

impl DetachedArtifactsCleanup {
    fn set_package(&mut self, package: PathBuf) {
        self.package = Some(package);
    }
}

impl Drop for DetachedArtifactsCleanup {
    fn drop(&mut self) {
        if let Some(package) = &self.package {
            let _ = std::fs::remove_file(package);
        }
        let _ = spawn_cleanup_process(self);
        if let Some(package) = &self.package {
            schedule_delete_on_reboot(package);
        }
        schedule_delete_on_reboot(&self.helper);
        schedule_delete_on_reboot(&self.directory);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ApplyUpdateArgs {
    target_version: String,
    package_path: PathBuf,
    install_root: PathBuf,
    old_pid: u32,
    readiness_handle: usize,
}

fn helper_version() -> &'static str {
    APP_VERSION_TEXT.trim()
}

fn parse_apply_update_args<I>(args: I) -> Result<ApplyUpdateArgs, String>
where
    I: IntoIterator<Item = OsString>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    if args.get(1).and_then(|value| value.to_str()) != Some(APPLY_UPDATE_FLAG) || args.len() != 7 {
        return Err("Rust Windows update helper received invalid arguments.".to_owned());
    }
    let target_version = args[2]
        .to_str()
        .ok_or_else(|| {
            "Rust Windows update helper received a non-UTF-8 target version.".to_owned()
        })?
        .to_owned();
    if !is_canonical_manifest_version(&target_version) {
        return Err("Rust Windows update helper refused a non-canonical target version.".to_owned());
    }
    let package_path = PathBuf::from(&args[3]);
    let install_root = PathBuf::from(&args[4]);
    let old_pid = args[5]
        .to_str()
        .ok_or_else(|| "Rust Windows update helper received a non-UTF-8 process id.".to_owned())?
        .parse::<u32>()
        .map_err(|_| "Rust Windows update helper received an invalid process id.".to_owned())?;
    if old_pid == 0 || old_pid == std::process::id() {
        return Err("Rust Windows update helper received an invalid process id.".to_owned());
    }
    let readiness_handle = args[6]
        .to_str()
        .ok_or_else(|| {
            "Rust Windows update helper received a non-UTF-8 readiness handle.".to_owned()
        })?
        .parse::<usize>()
        .map_err(|_| {
            "Rust Windows update helper received an invalid readiness handle.".to_owned()
        })?;
    if readiness_handle == 0 || readiness_handle == usize::MAX {
        return Err("Rust Windows update helper received an invalid readiness handle.".to_owned());
    }
    Ok(ApplyUpdateArgs { target_version, package_path, install_root, old_pid, readiness_handle })
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("Could not inspect {label}: {error}"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(format!(
            "Rust Windows update helper refused a non-directory or reparse-point {label}."
        ));
    }
    std::fs::canonicalize(path).map_err(|error| format!("Could not resolve {label}: {error}"))
}

fn canonical_file(path: &Path, label: &str) -> Result<PathBuf, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("Could not inspect {label}: {error}"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(format!(
            "Rust Windows update helper refused a non-regular or reparse-point {label}."
        ));
    }
    std::fs::canonicalize(path).map_err(|error| format!("Could not resolve {label}: {error}"))
}

fn target_file_version(version: &str) -> Result<WindowsFileVersion, String> {
    let parts =
        version.split('.').map(str::parse::<u16>).collect::<Result<Vec<_>, _>>().map_err(|_| {
            "Target release version did not fit Windows FILEVERSION fields.".to_owned()
        })?;
    let [major, minor, patch] = parts.as_slice() else {
        return Err("Target release version was not MAJOR.MINOR.PATCH.".to_owned());
    };
    Ok(WindowsFileVersion { major: *major, minor: *minor, patch: *patch, revision: 0 })
}

fn fresh_staging_directory(base: &Path) -> Result<PathBuf, String> {
    let epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    for _ in 0..TEMP_ATTEMPTS {
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = base.join(format!("stage-{}-{epoch}-{counter}", std::process::id()));
        if !path.exists() {
            return Ok(path);
        }
    }
    Err("Could not allocate a fresh Windows update staging path.".to_owned())
}

fn fetch_and_authenticate_package(
    args: &ApplyUpdateArgs,
    package_path: &Path,
) -> Result<Vec<u8>, String> {
    let mut checker = UpdateChecker::new(WinInetUpdateTransport, UpdatePlatform::Windows, false);
    let release =
        checker.fetch_release_version(&args.target_version).map_err(|error| error.to_string())?;
    if !is_updater_version_allowed(&release, helper_version()) {
        return Err("This SMU update helper is too old for the requested release.".to_owned());
    }
    let asset = select_update_asset(&release, UpdatePlatform::Windows)
        .ok_or_else(|| "Requested release did not include a Windows update package.".to_owned())?;
    if package_path.file_name() != Some(OsStr::new(&asset.name)) {
        return Err(
            "Rust Windows update helper refused a package filename that did not match the release manifest."
                .to_owned(),
        );
    }
    read_verified_update_asset(&asset, package_path).map_err(|error| error.to_string())
}

fn detached_current_helper(old_pid: u32) -> Result<(PathBuf, PathBuf), String> {
    let current_helper = canonical_file(
        &std::env::current_exe()
            .map_err(|error| format!("Could not resolve update helper: {error}"))?,
        "the running update helper",
    )?;
    if current_helper.file_name() != Some(OsStr::new(HELPER_EXE)) {
        return Err("Rust Windows update helper refused an unexpected executable name.".to_owned());
    }
    let helper_directory = current_helper
        .parent()
        .ok_or_else(|| "Rust Windows update helper has no parent directory.".to_owned())?;
    let helper_directory = canonical_directory(helper_directory, "the detached update directory")?;
    let temp_root = std::fs::canonicalize(std::env::temp_dir())
        .map_err(|error| format!("Could not resolve the Windows temporary directory: {error}"))?;
    if !helper_directory.starts_with(&temp_root) {
        return Err(
            "Rust Windows update helper refused a detached directory outside the Windows temporary directory."
                .to_owned(),
        );
    }
    let expected_prefix = format!("smu-update-launch-{old_pid}-");
    let directory_name =
        helper_directory.file_name().and_then(|value| value.to_str()).ok_or_else(|| {
            "Rust Windows update helper received an invalid detached directory name.".to_owned()
        })?;
    if !directory_name.starts_with(&expected_prefix) {
        return Err("Rust Windows update helper refused an unexpected detached launch directory."
            .to_owned());
    }
    Ok((current_helper, helper_directory))
}

fn detached_package_path(args: &ApplyUpdateArgs, current_helper: &Path) -> Result<PathBuf, String> {
    let package = canonical_file(&args.package_path, "the downloaded Windows update package")?;
    let helper_directory = current_helper
        .parent()
        .ok_or_else(|| "Rust Windows update helper has no parent directory.".to_owned())?;
    if package.parent() != Some(helper_directory) {
        return Err(
            "Rust Windows update helper refused a package outside its detached launch directory."
                .to_owned(),
        );
    }
    Ok(package)
}

fn validate_versions_and_signers(
    args: &ApplyUpdateArgs,
    staged_root: &Path,
    install_root: &Path,
    current_helper: &Path,
) -> Result<(), String> {
    let installed_app =
        canonical_file(&install_root.join(PRODUCT_EXE), "the installed SMU executable")?;
    let staged_app = canonical_file(&staged_root.join(PRODUCT_EXE), "the staged SMU executable")?;
    let staged_helper = canonical_file(&staged_root.join(HELPER_EXE), "the staged update helper")?;

    // The detached helper must still be trusted under the installed SMU release signer.
    // Require signer continuity for our own executable images, but never re-sign upstream DLLs.
    // Instead, the interim policy strictly pins their hashes to the installed libraries.
    require_matching_authenticode_signer(&installed_app, current_helper)
        .map_err(|error| error.to_string())?;
    for relative in WINDOWS_RELEASE_SIGNER_FILES {
        let staged = canonical_file(
            &staged_root.join(relative),
            &format!("the staged release-signed runtime file {relative}"),
        )?;
        require_matching_authenticode_signer(&installed_app, &staged).map_err(|error| {
            format!("Windows update runtime signer validation failed for {relative}: {error}")
        })?;
    }

    // SignPath Foundation does not permit us to re-sign upstream DLLs. Until a signed
    // library-update policy exists, accept only exact-byte matches to installed DLLs.
    // Both paths are canonicalized as regular, non-reparse files before hashing.
    // These checks run before readiness.signal() and replacement.
    for relative in WINDOWS_UNCHANGED_UPSTREAM_LIBRARY_FILES {
        let installed = canonical_file(
            &install_root.join(relative),
            &format!("the installed upstream runtime library {relative}"),
        )?;
        let staged = canonical_file(
            &staged_root.join(relative),
            &format!("the staged upstream runtime library {relative}"),
        )?;
        require_unchanged_windows_upstream_library(&installed, &staged).map_err(|error| {
            format!(
                "Windows update upstream library digest validation failed for {relative}: {error}"
            )
        })?;
    }

    // Do not re-sign the WinDivert kernel driver with the SMU certificate. Preserve its upstream kernel-compatible
    // signature instead by requiring signer continuity with the already installed driver.
    for relative in WINDOWS_UPSTREAM_SIGNER_CONTINUITY_FILES {
        let installed = canonical_file(
            &install_root.join(relative),
            &format!("the installed upstream-signed runtime file {relative}"),
        )?;
        let staged = canonical_file(
            &staged_root.join(relative),
            &format!("the staged upstream-signed runtime file {relative}"),
        )?;
        require_matching_authenticode_signer(&installed, &staged).map_err(|error| {
            format!("Windows update upstream signer continuity failed for {relative}: {error}")
        })?;
    }

    let expected = target_file_version(&args.target_version)?;
    let installed = windows_file_version(&installed_app).map_err(|error| error.to_string())?;
    let staged_app_version =
        windows_file_version(&staged_app).map_err(|error| error.to_string())?;
    let staged_helper_version =
        windows_file_version(&staged_helper).map_err(|error| error.to_string())?;
    if expected <= installed {
        return Err("Rust Windows update helper refused a non-upgrade target version.".to_owned());
    }
    if staged_app_version != expected || staged_helper_version != expected {
        return Err(
            "Authenticated Windows update bundle FILEVERSION did not match its release tag."
                .to_owned(),
        );
    }
    Ok(())
}

fn open_expected_old_process(
    pid: u32,
    installed_app: &Path,
) -> Result<Option<ProcessHandle>, String> {
    // SAFETY: OpenProcess receives only value arguments and returns an owned kernel handle on success.
    let process = unsafe {
        OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_DUP_HANDLE,
            0,
            pid,
        )
    };
    if process.is_null() {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER.cast_signed()) {
            return Ok(None);
        }
        return Err(format!(
            "Rust Windows update helper could not open process {pid} for validation: {error}"
        ));
    }
    let process = ProcessHandle(process);
    let mut image_buffer = vec![0_u16; 32_768];
    let mut image_length = u32::try_from(image_buffer.len()).unwrap_or(u32::MAX);
    // SAFETY: the process handle is live, the UTF-16 buffer is writable, and image_length describes it.
    if unsafe {
        QueryFullProcessImageNameW(process.0, 0, image_buffer.as_mut_ptr(), &raw mut image_length)
    } == 0
    {
        return Err(format!(
            "Rust Windows update helper could not resolve process {pid}: {}",
            std::io::Error::last_os_error()
        ));
    }
    let image_length = usize::try_from(image_length)
        .map_err(|_| "SMU process image path length was invalid.".to_owned())?;
    let process_image = PathBuf::from(OsString::from_wide(&image_buffer[..image_length]));
    let process_image = canonical_file(&process_image, "the running SMU process executable")?;
    if process_image != installed_app {
        return Err(format!(
            "Rust Windows update helper refused process {pid} because it is not the installed SMU executable."
        ));
    }
    Ok(Some(process))
}

fn wait_for_old_process(process: Option<&ProcessHandle>, pid: u32) -> Result<(), String> {
    let Some(process) = process else {
        return Ok(());
    };
    let timeout_ms = u32::try_from(OLD_PROCESS_WAIT.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: the process handle remains live for the synchronous wait and pins the original process object.
    match unsafe { WaitForSingleObject(process.0, timeout_ms) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => Err(format!("Timed out waiting for SMU process {pid} to exit.")),
        _ => Err(format!("Failed while waiting for SMU process {pid} to exit.")),
    }
}

fn launch_updated_app(install_root: &Path) -> Result<Child, String> {
    Command::new(install_root.join(PRODUCT_EXE))
        .current_dir(install_root)
        .spawn()
        .map_err(|error| format!("Could not relaunch the updated SMU executable: {error}"))
}

fn require_startup_survival(child: &mut Child) -> Result<(), String> {
    std::thread::sleep(RELAUNCH_SURVIVAL);
    match child.try_wait() {
        Ok(None) => Ok(()),
        Ok(Some(status)) => Err(format!("Updated SMU exited during startup with status {status}.")),
        Err(error) => Err(format!("Could not verify updated SMU startup: {error}")),
    }
}

fn rollback_and_relaunch_previous(
    transaction: WindowsReplacementTransaction,
    install_root: &Path,
    primary_error: &str,
) -> String {
    if let Err(rollback) = transaction.rollback() {
        return format!("{primary_error} Rollback also failed: {rollback}");
    }
    let mut restored = match launch_updated_app(install_root) {
        Ok(child) => child,
        Err(error) => {
            return format!(
                "{primary_error} The previous SMU version was restored, but could not be relaunched: {error}"
            );
        }
    };
    match require_startup_survival(&mut restored) {
        Ok(()) => format!(
            "{primary_error} The previous SMU version was restored and relaunched successfully."
        ),
        Err(error) => format!(
            "{primary_error} The previous SMU version was restored, but its relaunch failed: {error}"
        ),
    }
}

fn apply(args: &ApplyUpdateArgs) -> Result<(), String> {
    let (current_helper, detached_directory) = detached_current_helper(args.old_pid)?;
    let mut cleanup = DetachedArtifactsCleanup {
        helper: current_helper.clone(),
        package: None,
        directory: detached_directory,
    };
    let install_root = canonical_directory(&args.install_root, "the SMU installation directory")?;
    if current_helper.starts_with(&install_root) {
        return Err(
            "Rust Windows update helper must run from a detached copy outside the installation directory."
                .to_owned(),
        );
    }
    let installed_app =
        canonical_file(&install_root.join(PRODUCT_EXE), "the installed SMU executable")?;
    let old_process = open_expected_old_process(args.old_pid, &installed_app)?;
    let old_process_for_readiness = old_process.as_ref().ok_or_else(|| {
        "Rust Windows update helper lost the parent process before duplicating readiness."
            .to_owned()
    })?;
    let readiness =
        ReadyEvent::duplicate_from_parent(old_process_for_readiness, args.readiness_handle)?;
    let package_path = detached_package_path(args, &current_helper)?;
    cleanup.set_package(package_path.clone());
    let package_result = fetch_and_authenticate_package(args, &package_path);
    let _ = std::fs::remove_file(&package_path);
    let package = package_result?;
    let authenticated_hashes =
        windows_bundle_file_sha256(&package).map_err(|error| error.to_string())?;
    // Keep staging under the parent-owned detached launch directory. If the parent times out before
    // readiness and terminates this helper, its DetachedLaunchDirectory drop can remove the entire
    // launch tree even though forced termination skips this process's Rust destructors.
    let staging = fresh_staging_directory(&cleanup.directory)?;
    let staged_root =
        extract_windows_bundle(&package, &staging).map_err(|error| error.to_string())?;
    let result = (|| {
        let mut prepared = prepare_windows_bundle_tree(&staged_root, &authenticated_hashes)
            .map_err(|error| error.to_string())?;
        validate_versions_and_signers(args, prepared.root(), &install_root, &current_helper)?;
        // Final parent handoff boundary: the old process object is already pinned and every authenticated
        // package/staging/signer/version check is complete. Only now may the parent safely exit.
        readiness.signal()?;
        wait_for_old_process(old_process.as_ref(), args.old_pid)?;
        let backup = install_root.join(format!(".smu-update-backup-{}", std::process::id()));
        let transaction = replace_windows_bundle_tree(&mut prepared, &install_root, &backup)
            .map_err(|error| error.to_string())?;
        let mut child = match launch_updated_app(&install_root) {
            Ok(child) => child,
            Err(error) => {
                return Err(rollback_and_relaunch_previous(transaction, &install_root, &error));
            }
        };
        if let Err(error) = require_startup_survival(&mut child) {
            // If try_wait itself failed, the new process may still be live; stop it before restoring files.
            let _ = child.kill();
            let _ = child.wait();
            return Err(rollback_and_relaunch_previous(transaction, &install_root, &error));
        }
        transaction.commit().map_err(|error| error.to_string())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

pub fn run() -> Result<(), String> {
    let args = parse_apply_update_args(std::env::args_os())?;
    apply(&args)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn helper_requires_exact_apply_arguments_and_canonical_target_version() {
        let parsed = parse_apply_update_args(args(&[
            "helper.exe",
            APPLY_UPDATE_FLAG,
            "3.5.0",
            "C:/Temp/update.zip",
            "C:/SMU",
            "42",
            "12345",
        ]))
        .expect("valid helper arguments should parse");
        assert_eq!(parsed.target_version, "3.5.0");
        assert_eq!(parsed.old_pid, 42);
        assert_eq!(parsed.readiness_handle, 12_345);
        assert!(parse_apply_update_args(args(&["helper.exe"])).is_err());
        assert!(
            parse_apply_update_args(args(&[
                "helper.exe",
                APPLY_UPDATE_FLAG,
                "V3.5.0",
                "update.zip",
                "C:/SMU",
                "42",
                "12345",
            ]))
            .is_err()
        );
    }

    #[test]
    fn target_windows_file_version_uses_three_canonical_components_and_zero_revision() {
        assert_eq!(
            target_file_version("3.5.7").expect("version should parse").as_tuple(),
            (3, 5, 7, 0)
        );
        assert!(target_file_version("65536.1.1").is_err());
    }

    #[test]
    fn helper_version_comes_from_repository_version_file() {
        assert!(is_canonical_manifest_version(helper_version()));
    }
}
