use smu_updater::{
    UpdateChecker, UpdatePlatform, WinInetUpdateTransport, WindowsFileVersion,
    extract_windows_bundle, is_canonical_manifest_version, is_updater_version_allowed,
    prepare_windows_bundle_tree, read_verified_update_asset, replace_windows_bundle_tree,
    require_matching_authenticode_signer, select_update_asset, windows_bundle_file_sha256,
    windows_file_version,
};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        QueryFullProcessImageNameW, WaitForSingleObject,
    },
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

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns a process handle returned by OpenProcess.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ApplyUpdateArgs {
    target_version: String,
    package_path: PathBuf,
    install_root: PathBuf,
    old_pid: u32,
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
    Ok(ApplyUpdateArgs { target_version, package_path, install_root, old_pid })
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    if !path.is_dir() {
        return Err(format!("{label} does not exist: {}", path.display()));
    }
    std::fs::canonicalize(path).map_err(|error| format!("Could not resolve {label}: {error}"))
}

fn canonical_file(path: &Path, label: &str) -> Result<PathBuf, String> {
    if !path.is_file() {
        return Err(format!("{label} does not exist: {}", path.display()));
    }
    std::fs::canonicalize(path).map_err(|error| format!("Could not resolve {label}: {error}"))
}

fn target_file_version(version: &str) -> Result<WindowsFileVersion, String> {
    let parts =
        version.split('.').map(|part| part.parse::<u16>()).collect::<Result<Vec<_>, _>>().map_err(
            |_| "Target release version did not fit Windows FILEVERSION fields.".to_owned(),
        )?;
    let [major, minor, patch] = parts.as_slice() else {
        return Err("Target release version was not MAJOR.MINOR.PATCH.".to_owned());
    };
    Ok(WindowsFileVersion { major: *major, minor: *minor, patch: *patch, revision: 0 })
}

fn fresh_staging_directory() -> Result<PathBuf, String> {
    let base = std::env::temp_dir();
    let epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    for _ in 0..TEMP_ATTEMPTS {
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = base.join(format!("smu-update-stage-{}-{epoch}-{counter}", std::process::id()));
        if !path.exists() {
            return Ok(path);
        }
    }
    Err("Could not allocate a fresh Windows update staging path.".to_owned())
}

fn fetch_and_authenticate_package(args: &ApplyUpdateArgs) -> Result<Vec<u8>, String> {
    let mut checker = UpdateChecker::new(WinInetUpdateTransport, UpdatePlatform::Windows, false);
    let release =
        checker.fetch_release_version(&args.target_version).map_err(|error| error.to_string())?;
    if !is_updater_version_allowed(&release, helper_version()) {
        return Err("This SMU update helper is too old for the requested release.".to_owned());
    }
    let asset = select_update_asset(&release, UpdatePlatform::Windows)
        .ok_or_else(|| "Requested release did not include a Windows update package.".to_owned())?;
    read_verified_update_asset(&asset, &args.package_path).map_err(|error| error.to_string())
}

fn detached_current_helper(install_root: &Path) -> Result<PathBuf, String> {
    let current_helper = canonical_file(
        &std::env::current_exe()
            .map_err(|error| format!("Could not resolve update helper: {error}"))?,
        "the running update helper",
    )?;
    if current_helper.starts_with(install_root) {
        return Err(
            "Rust Windows update helper must run from a detached copy outside the installation directory."
                .to_owned(),
        );
    }
    Ok(current_helper)
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

    require_matching_authenticode_signer(&installed_app, &current_helper)
        .map_err(|error| error.to_string())?;
    require_matching_authenticode_signer(&installed_app, &staged_app)
        .map_err(|error| error.to_string())?;
    require_matching_authenticode_signer(&installed_app, &staged_helper)
        .map_err(|error| error.to_string())?;

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
    let process =
        unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
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
        QueryFullProcessImageNameW(process.0, 0, image_buffer.as_mut_ptr(), &mut image_length)
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

fn apply(args: ApplyUpdateArgs) -> Result<(), String> {
    let install_root = canonical_directory(&args.install_root, "the SMU installation directory")?;
    let installed_app =
        canonical_file(&install_root.join(PRODUCT_EXE), "the installed SMU executable")?;
    let old_process = open_expected_old_process(args.old_pid, &installed_app)?;
    let current_helper = detached_current_helper(&install_root)?;
    let package = fetch_and_authenticate_package(&args)?;
    let authenticated_hashes =
        windows_bundle_file_sha256(&package).map_err(|error| error.to_string())?;
    let staging = fresh_staging_directory()?;
    let staged_root =
        extract_windows_bundle(&package, &staging).map_err(|error| error.to_string())?;
    let result = (|| {
        let mut prepared = prepare_windows_bundle_tree(&staged_root, &authenticated_hashes)
            .map_err(|error| error.to_string())?;
        validate_versions_and_signers(&args, prepared.root(), &install_root, &current_helper)?;
        wait_for_old_process(old_process.as_ref(), args.old_pid)?;
        let backup = install_root.join(format!(".smu-update-backup-{}", std::process::id()));
        let transaction = replace_windows_bundle_tree(&mut prepared, &install_root, &backup)
            .map_err(|error| error.to_string())?;
        let mut child = match launch_updated_app(&install_root) {
            Ok(child) => child,
            Err(error) => {
                let rollback = transaction.rollback().map_err(|rollback| rollback.to_string());
                return rollback.map_or_else(
                    |rollback| Err(format!("{error} Rollback also failed: {rollback}")),
                    |()| Err(error),
                );
            }
        };
        if let Err(error) = require_startup_survival(&mut child) {
            let rollback = transaction.rollback().map_err(|rollback| rollback.to_string());
            return rollback.map_or_else(
                |rollback| Err(format!("{error} Rollback also failed: {rollback}")),
                |()| Err(error),
            );
        }
        transaction.commit().map_err(|error| error.to_string())
    })();
    if result.is_ok() {
        let _ = std::fs::remove_file(&args.package_path);
    }
    let _ = std::fs::remove_dir_all(&staging);
    result
}

pub fn run() -> Result<(), String> {
    let args = parse_apply_update_args(std::env::args_os())?;
    apply(args)
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
        ]))
        .expect("valid helper arguments should parse");
        assert_eq!(parsed.target_version, "3.5.0");
        assert_eq!(parsed.old_pid, 42);
        assert!(parse_apply_update_args(args(&["helper.exe"])).is_err());
        assert!(
            parse_apply_update_args(args(&[
                "helper.exe",
                APPLY_UPDATE_FLAG,
                "V3.5.0",
                "update.zip",
                "C:/SMU",
                "42",
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
