//! Linux `AppImage` replacement path matching the C++ updater safety contract.

use crate::{
    CurlUpdateTransport, LinuxArchitecture, MAXIMUM_UPDATE_BYTES, ReleaseAsset, ReleaseInfo,
    UpdatePlatform, current_platform, is_updater_version_allowed, select_update_asset,
};
use sha2::{Digest as _, Sha256};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use zip::ZipArchive;

const TRUSTED_RELEASE_ASSET_PREFIX: &str =
    "https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/download/";
const STICKY_BIT: u32 = 0o1000;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinuxUpdateError(String);

impl LinuxUpdateError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for LinuxUpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for LinuxUpdateError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinuxUpdateLaunch {
    pub helper_pid: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LinuxAssetKind {
    Unsupported,
    AppImage,
    DistributionBundle,
}

/// C++ `IsAutoApplySupported` for Linux.
#[must_use]
pub fn linux_auto_apply_supported() -> bool {
    resolve_replaceable_appimage().is_ok()
}

/// Validate, stage, and launch the detached Linux `AppImage` replacement helper.
///
/// The caller should begin normal application shutdown after this returns successfully. The helper
/// waits for the current PID to exit before swapping files and performs the same rollback checks as
/// the C++ updater.
///
/// # Errors
///
/// Returns a user-facing compatibility/safety/download/staging failure without modifying the live
/// `AppImage`.
pub fn apply_linux_update(
    release: &ReleaseInfo,
    local_version: &str,
) -> Result<LinuxUpdateLaunch, LinuxUpdateError> {
    if !is_updater_version_allowed(release, local_version) {
        return Err(LinuxUpdateError::new(
            "Updater refused to apply a release without a compatible V3.4 update manifest.",
        ));
    }
    let asset = select_update_asset(release, current_platform()).ok_or_else(|| {
        LinuxUpdateError::new("No matching update package asset was found for this platform.")
    })?;
    if !is_trusted_release_asset(&asset) {
        return Err(LinuxUpdateError::new(
            "Updater refused an untrusted or oversized release asset.",
        ));
    }
    apply_linux_asset(&asset, &release.version)
}

fn apply_linux_asset(
    asset: &ReleaseAsset,
    new_version: &str,
) -> Result<LinuxUpdateLaunch, LinuxUpdateError> {
    let lower_name = asset.name.to_ascii_lowercase();
    let kind = linux_asset_kind(&lower_name);
    let architecture = current_linux_architecture();
    if kind == LinuxAssetKind::Unsupported
        || asset_targets_different_linux_architecture(&lower_name, architecture)
    {
        return Err(LinuxUpdateError::new(
            "Linux auto-update requires a matching AppImage or Linux distribution bundle.",
        ));
    }

    let current_appimage = resolve_replaceable_appimage()?;
    let appimage_dir = current_appimage
        .parent()
        .ok_or_else(|| LinuxUpdateError::new("Could not resolve the current AppImage path."))?;
    let staged = create_unique_file(
        appimage_dir,
        &format!(
            ".{}.update",
            current_appimage.file_name().and_then(|value| value.to_str()).unwrap_or("SMU.AppImage")
        ),
    )?;

    let stage_result = match kind {
        LinuxAssetKind::AppImage => {
            download_asset_to_file(asset, &staged).and_then(|()| validate_appimage_file(&staged))
        }
        LinuxAssetKind::DistributionBundle => {
            stage_appimage_from_bundle(asset, new_version, &staged)
        }
        LinuxAssetKind::Unsupported => unreachable!(),
    };
    if let Err(error) = stage_result {
        let _ = fs::remove_file(&staged);
        return Err(error);
    }

    if !staged.is_file()
        || staged.metadata().map(|metadata| metadata.len()).unwrap_or_default() == 0
    {
        let _ = fs::remove_file(&staged);
        return Err(LinuxUpdateError::new("Downloaded AppImage was missing or empty."));
    }
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o755)).map_err(|error| {
        let _ = fs::remove_file(&staged);
        LinuxUpdateError::new(format!("Failed to make downloaded AppImage executable: {error}"))
    })?;

    launch_updater_script(&current_appimage, &staged)
}

fn linux_asset_kind(lower_name: &str) -> LinuxAssetKind {
    if lower_name.ends_with(".appimage") {
        LinuxAssetKind::AppImage
    } else if Path::new(lower_name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
        && lower_name.contains("linux")
        && !["windows", "macos", "darwin"].iter().any(|needle| lower_name.contains(needle))
    {
        LinuxAssetKind::DistributionBundle
    } else {
        LinuxAssetKind::Unsupported
    }
}

const fn current_linux_architecture() -> LinuxArchitecture {
    match current_platform() {
        UpdatePlatform::Linux(architecture) => architecture,
        _ => LinuxArchitecture::Unknown,
    }
}

fn asset_targets_different_linux_architecture(
    lower_name: &str,
    architecture: LinuxArchitecture,
) -> bool {
    match architecture {
        LinuxArchitecture::X86_64 => lower_name.contains("aarch64") || lower_name.contains("arm64"),
        LinuxArchitecture::AArch64 => {
            ["x86_64", "x86-64", "amd64", "x64"].iter().any(|needle| lower_name.contains(needle))
        }
        LinuxArchitecture::Unknown => false,
    }
}

fn is_trusted_release_asset(asset: &ReleaseAsset) -> bool {
    asset.size_bytes > 0
        && asset.size_bytes <= MAXIMUM_UPDATE_BYTES
        && asset.sha256.len() == 64
        && asset.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        && asset.download_url.starts_with(TRUSTED_RELEASE_ASSET_PREFIX)
}

fn resolve_replaceable_appimage() -> Result<PathBuf, LinuxUpdateError> {
    let appimage_env =
        std::env::var_os("APPIMAGE").filter(|value| !value.is_empty()).ok_or_else(|| {
            LinuxUpdateError::new(
                "Linux auto-update requires running SMU from an AppImage. APPIMAGE is not set.",
            )
        })?;
    let appdir_env =
        std::env::var_os("APPDIR").filter(|value| !value.is_empty()).ok_or_else(|| {
            LinuxUpdateError::new(
                "Linux auto-update requires the APPDIR supplied by the AppImage runtime.",
            )
        })?;

    let appdir = fs::canonicalize(appdir_env).map_err(|_| {
        LinuxUpdateError::new("Could not resolve the current AppImage mount directory.")
    })?;
    let executable = fs::canonicalize("/proc/self/exe").map_err(|_| {
        LinuxUpdateError::new(
            "APPIMAGE is set, but this SMU executable is not running from its AppImage mount.",
        )
    })?;
    if !path_is_within(&executable, &appdir) {
        return Err(LinuxUpdateError::new(
            "APPIMAGE is set, but this SMU executable is not running from its AppImage mount.",
        ));
    }

    let appimage = absolute_path(Path::new(&appimage_env))?;
    let effective_uid = effective_uid().ok_or_else(|| {
        LinuxUpdateError::new("Could not inspect the current AppImage location before updating.")
    })?;
    validate_replaceable_appimage(&appimage, effective_uid)
}

fn absolute_path(path: &Path) -> Result<PathBuf, LinuxUpdateError> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::env::current_dir()
        .map(|current| current.join(path))
        .map_err(|_| LinuxUpdateError::new("Could not resolve the current AppImage path."))
}

fn validate_replaceable_appimage(
    current_appimage: &Path,
    effective_uid: u32,
) -> Result<PathBuf, LinuxUpdateError> {
    let metadata = current_appimage.metadata().map_err(|_| {
        LinuxUpdateError::new(format!(
            "Current AppImage path is missing or is not a regular file: {}",
            current_appimage.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(LinuxUpdateError::new(format!(
            "Current AppImage path is missing or is not a regular file: {}",
            current_appimage.display()
        )));
    }
    let directory = current_appimage
        .parent()
        .ok_or_else(|| LinuxUpdateError::new("Could not resolve the current AppImage path."))?;
    let directory_metadata = directory.metadata().map_err(|_| {
        LinuxUpdateError::new(format!(
            "Cannot update AppImage because its folder is not writable: {}",
            directory.display()
        ))
    })?;
    if !directory_metadata.is_dir() {
        return Err(LinuxUpdateError::new(format!(
            "Cannot update AppImage because its folder is not writable: {}",
            directory.display()
        )));
    }

    if directory_metadata.mode() & STICKY_BIT != 0
        && effective_uid != 0
        && effective_uid != directory_metadata.uid()
        && effective_uid != metadata.uid()
    {
        return Err(LinuxUpdateError::new(
            "Cannot replace this AppImage because it is owned by another user in a protected folder.",
        ));
    }

    let probe = create_unique_file(
        directory,
        &format!(
            ".{}.update",
            current_appimage.file_name().and_then(|value| value.to_str()).unwrap_or("SMU.AppImage")
        ),
    )
    .map_err(|_| {
        LinuxUpdateError::new(format!(
            "Cannot update AppImage because its folder is not writable: {}",
            directory.display()
        ))
    })?;
    fs::remove_file(&probe).map_err(|error| {
        LinuxUpdateError::new(format!(
            "Could not clean up the AppImage update writeability probe: {error}"
        ))
    })?;
    Ok(current_appimage.to_path_buf())
}

fn path_is_within(path: &Path, directory: &Path) -> bool {
    path.strip_prefix(directory).is_ok_and(|remainder| remainder.components().next().is_some())
}

fn effective_uid() -> Option<u32> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("Uid:"))?;
    line.split_ascii_whitespace().nth(2)?.parse().ok()
}

fn create_unique_file(directory: &Path, stem: &str) -> Result<PathBuf, LinuxUpdateError> {
    for _ in 0..128 {
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!("{stem}-{}-{counter}", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(LinuxUpdateError::new(format!(
                    "Failed to create a temporary AppImage: {error}"
                )));
            }
        }
    }
    Err(LinuxUpdateError::new("Failed to create a temporary AppImage."))
}

fn download_asset_to_file(
    asset: &ReleaseAsset,
    destination: &Path,
) -> Result<(), LinuxUpdateError> {
    if !is_trusted_release_asset(asset) {
        return Err(LinuxUpdateError::new(
            "Updater refused an untrusted or oversized release asset.",
        ));
    }
    CurlUpdateTransport::default()
        .download_to_file(&asset.download_url, destination)
        .map_err(|error| LinuxUpdateError::new(error.to_string()))?;
    verify_downloaded_asset(asset, destination)
}

fn verify_downloaded_asset(
    asset: &ReleaseAsset,
    destination: &Path,
) -> Result<(), LinuxUpdateError> {
    let size = destination.metadata().map(|metadata| metadata.len()).unwrap_or_default();
    if size != asset.size_bytes {
        let _ = fs::remove_file(destination);
        return Err(LinuxUpdateError::new(
            "Downloaded update size did not match update-manifest.json.",
        ));
    }
    let actual = sha256_file(destination)?;
    if !actual.eq_ignore_ascii_case(&asset.sha256) {
        let _ = fs::remove_file(destination);
        return Err(LinuxUpdateError::new(
            "Downloaded update SHA-256 did not match update-manifest.json.",
        ));
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, LinuxUpdateError> {
    let mut file = File::open(path).map_err(|_| {
        LinuxUpdateError::new("Could not open downloaded update for SHA-256 verification.")
    })?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 65_536];
    loop {
        let count = file.read(&mut buffer).map_err(|_| {
            LinuxUpdateError::new(
                "Failed while reading the downloaded update for SHA-256 verification.",
            )
        })?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn validate_appimage_file(path: &Path) -> Result<(), LinuxUpdateError> {
    let mut file = File::open(path).map_err(|_| {
        LinuxUpdateError::new(
            "Downloaded update was not a compatible AppImage for this Linux architecture.",
        )
    })?;
    let mut header = [0_u8; 20];
    file.read_exact(&mut header).map_err(|_| {
        LinuxUpdateError::new(
            "Downloaded update was not a compatible AppImage for this Linux architecture.",
        )
    })?;
    if !has_expected_appimage_header(&header) {
        return Err(LinuxUpdateError::new(
            "Downloaded update was not a compatible AppImage for this Linux architecture.",
        ));
    }
    Ok(())
}

fn has_expected_appimage_header(bytes: &[u8]) -> bool {
    if bytes.len() < 20 || bytes[..6] != [0x7f, b'E', b'L', b'F', 2, 1] {
        return false;
    }
    let expected = current_elf_machine();
    if expected == 0 {
        return true;
    }
    let actual = u16::from(bytes[18]) | (u16::from(bytes[19]) << 8);
    actual == expected
}

const fn current_elf_machine() -> u16 {
    #[cfg(target_arch = "x86_64")]
    {
        return 62;
    }
    #[cfg(target_arch = "aarch64")]
    {
        return 183;
    }
    #[allow(unreachable_code)]
    0
}

fn stage_appimage_from_bundle(
    asset: &ReleaseAsset,
    new_version: &str,
    staged_appimage: &Path,
) -> Result<(), LinuxUpdateError> {
    let package_path = create_unique_file(std::env::temp_dir().as_path(), "smu-update")?;
    let result = download_asset_to_file(asset, &package_path).and_then(|()| {
        extract_linux_appimage_from_package(
            &package_path,
            &asset.name,
            new_version,
            staged_appimage,
        )
    });
    let _ = fs::remove_file(&package_path);
    result
}

fn extract_linux_appimage_from_package(
    package_path: &Path,
    bundle_asset_name: &str,
    new_version: &str,
    destination: &Path,
) -> Result<(), LinuxUpdateError> {
    let candidates = appimage_names_for_current_architecture(bundle_asset_name, new_version);
    if candidates.is_empty() {
        return Err(LinuxUpdateError::new(
            "Linux auto-update does not recognize this CPU architecture.",
        ));
    }
    let file = File::open(package_path).map_err(|_| {
        LinuxUpdateError::new(
            "The Linux update bundle did not contain the expected root-level AppImage for this architecture.",
        )
    })?;
    let mut archive = ZipArchive::new(file).map_err(|_| {
        LinuxUpdateError::new(
            "The Linux update bundle did not contain the expected root-level AppImage for this architecture.",
        )
    })?;
    for candidate in candidates {
        let Ok(mut entry) = archive.by_name(&candidate) else {
            continue;
        };
        if entry.is_dir()
            || entry.encrypted()
            || entry.size() == 0
            || entry.size() > MAXIMUM_UPDATE_BYTES
        {
            return Err(LinuxUpdateError::new(
                "The requested update file was empty, encrypted, or too large.",
            ));
        }
        let mut output = File::create(destination)
            .map_err(|_| LinuxUpdateError::new("Failed to create the temporary AppImage."))?;
        std::io::copy(&mut entry, &mut output).map_err(|_| {
            LinuxUpdateError::new(format!("Failed to extract {candidate} from the update package."))
        })?;
        output.flush().map_err(|_| {
            LinuxUpdateError::new("Failed to finish writing the temporary AppImage.")
        })?;
        return validate_appimage_file(destination).map_err(|_| {
            LinuxUpdateError::new(
                "The Linux update bundle contained an incompatible or invalid AppImage.",
            )
        });
    }
    Err(LinuxUpdateError::new(
        "The Linux update bundle did not contain the expected root-level AppImage for this architecture.",
    ))
}

fn appimage_names_for_current_architecture(
    bundle_asset_name: &str,
    new_version: &str,
) -> Vec<String> {
    let architecture = match current_linux_architecture() {
        LinuxArchitecture::X86_64 => "x86_64",
        LinuxArchitecture::AArch64 => "aarch64",
        LinuxArchitecture::Unknown => return Vec::new(),
    };
    let mut names = Vec::new();
    if bundle_asset_name.to_ascii_lowercase().ends_with(".zip") && bundle_asset_name.len() > 4 {
        names.push(format!("{}.AppImage", &bundle_asset_name[..bundle_asset_name.len() - 4]));
    }
    if !new_version.is_empty() {
        names.push(format!("Spencer-Macro-Utilities-V{new_version}-Linux-{architecture}.AppImage"));
    }
    names.push(format!("Spencer-Macro-Utilities-{architecture}.AppImage"));
    match current_linux_architecture() {
        LinuxArchitecture::X86_64 => {
            names.push("Spencer-Macro-Utilities-amd64.AppImage".to_owned());
        }
        LinuxArchitecture::AArch64 => {
            names.push("Spencer-Macro-Utilities-arm64.AppImage".to_owned());
        }
        LinuxArchitecture::Unknown => {}
    }
    names.dedup();
    names
}

fn launch_updater_script(
    current_appimage: &Path,
    staged_appimage: &Path,
) -> Result<LinuxUpdateLaunch, LinuxUpdateError> {
    let script = create_unique_file(std::env::temp_dir().as_path(), "smu-appimage-updater")?;
    let write_result = fs::write(&script, build_linux_updater_script());
    if let Err(error) = write_result {
        let _ = fs::remove_file(staged_appimage);
        let _ = fs::remove_file(&script);
        return Err(LinuxUpdateError::new(format!(
            "Failed to open updater script for writing: {error}"
        )));
    }
    if let Err(error) = fs::set_permissions(&script, fs::Permissions::from_mode(0o700)) {
        let _ = fs::remove_file(staged_appimage);
        let _ = fs::remove_file(&script);
        return Err(LinuxUpdateError::new(format!(
            "Failed to make updater script executable: {error}"
        )));
    }

    let child = Command::new("/bin/sh")
        .arg(&script)
        .arg(current_appimage)
        .arg(staged_appimage)
        .arg(std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|error| {
            let _ = fs::remove_file(staged_appimage);
            let _ = fs::remove_file(&script);
            LinuxUpdateError::new(format!("Failed to launch updater script: {error}"))
        })?;
    Ok(LinuxUpdateLaunch { helper_pid: child.id() })
}

const fn build_linux_updater_script() -> &'static str {
    r#"#!/bin/sh
set -u

OLD_APPIMAGE="$1"
NEW_APPIMAGE="$2"
OLD_PID="$3"
LOG_FILE="${TMPDIR:-/tmp}/smu-appimage-updater.log"

echo "SMU AppImage updater started" > "$LOG_FILE"
echo "Old AppImage: $OLD_APPIMAGE" >> "$LOG_FILE"
echo "New AppImage: $NEW_APPIMAGE" >> "$LOG_FILE"
echo "Old PID: $OLD_PID" >> "$LOG_FILE"

relaunch_previous() {
    if kill -0 "$OLD_PID" 2>/dev/null; then
        echo "Previous SMU process is still running; not starting a duplicate" >> "$LOG_FILE"
        return
    fi
    if [ ! -f "$OLD_APPIMAGE" ] || [ ! -x "$OLD_APPIMAGE" ]; then
        echo "Previous AppImage is unavailable and could not be relaunched" >> "$LOG_FILE"
        return
    fi
    nohup "$OLD_APPIMAGE" >/dev/null 2>&1 &
}

cleanup_before_swap() {
    rm -f -- "$NEW_APPIMAGE" "$0"
    relaunch_previous
}

i=0
while kill -0 "$OLD_PID" 2>/dev/null; do
    i=$((i + 1))
    if [ "$i" -gt 100 ]; then
        echo "Timed out waiting for old SMU process to exit" >> "$LOG_FILE"
        cleanup_before_swap
        exit 1
    fi
    sleep 0.1
done

sleep 0.5

if ! chmod 755 "$NEW_APPIMAGE" 2>> "$LOG_FILE"; then
    cleanup_before_swap
    exit 1
fi

BACKUP_APPIMAGE="${OLD_APPIMAGE}.old.${OLD_PID}.$$"
if [ -e "$BACKUP_APPIMAGE" ]; then
    echo "Refusing to overwrite an existing AppImage backup" >> "$LOG_FILE"
    cleanup_before_swap
    exit 1
fi
i=0
while true; do
    if mv -- "$OLD_APPIMAGE" "$BACKUP_APPIMAGE" 2>> "$LOG_FILE"; then
        break
    fi

    i=$((i + 1))
    if [ "$i" -gt 40 ]; then
        echo "Failed to move the old AppImage aside" >> "$LOG_FILE"
        cleanup_before_swap
        exit 1
    fi

    sleep 0.25
done

if ! mv -- "$NEW_APPIMAGE" "$OLD_APPIMAGE" 2>> "$LOG_FILE"; then
    echo "Failed to install the new AppImage; restoring the previous version" >> "$LOG_FILE"
    if mv -- "$BACKUP_APPIMAGE" "$OLD_APPIMAGE" >> "$LOG_FILE" 2>&1; then
        relaunch_previous
    else
        echo "Automatic rollback failed; backup remains at $BACKUP_APPIMAGE" >> "$LOG_FILE"
    fi
    rm -f -- "$NEW_APPIMAGE"
    rm -f -- "$0"
    exit 1
fi

nohup "$OLD_APPIMAGE" >/dev/null 2>&1 &
NEW_PID=$!
sleep 2
if ! kill -0 "$NEW_PID" 2>/dev/null; then
    echo "Updated AppImage exited during startup; restoring the previous version" >> "$LOG_FILE"
    rm -f -- "$OLD_APPIMAGE"
    if mv -- "$BACKUP_APPIMAGE" "$OLD_APPIMAGE" >> "$LOG_FILE" 2>&1; then
        relaunch_previous
    else
        echo "Automatic rollback failed; backup remains at $BACKUP_APPIMAGE" >> "$LOG_FILE"
    fi
    rm -f -- "$0"
    exit 1
fi

rm -f -- "$BACKUP_APPIMAGE"
rm -f -- "$0"
exit 0
"#
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use sha2::Sha256;
    use std::io::Cursor;
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("smu-updater-{label}-{}-{counter}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("test directory should be created");
            Self { path }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn appimage_bytes() -> Vec<u8> {
        let mut bytes = vec![0_u8; 64];
        bytes[..6].copy_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1]);
        let machine = current_elf_machine();
        bytes[18] = u8::try_from(machine & 0xff).unwrap_or_default();
        bytes[19] = u8::try_from(machine >> 8).unwrap_or_default();
        bytes
    }

    fn zip_with_entry(name: &str, bytes: &[u8]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        writer.start_file(name, options).expect("zip entry should start");
        writer.write_all(bytes).expect("zip entry should write");
        writer.finish().expect("zip should finish").into_inner()
    }

    #[test]
    fn asset_kind_and_architecture_guards_match_cpp() {
        assert_eq!(linux_asset_kind("x.appimage"), LinuxAssetKind::AppImage);
        assert_eq!(linux_asset_kind("x-linux.zip"), LinuxAssetKind::DistributionBundle);
        assert_eq!(linux_asset_kind("x-windows-linux.zip"), LinuxAssetKind::Unsupported);
        assert_eq!(linux_asset_kind("x.zip"), LinuxAssetKind::Unsupported);
        let foreign = match current_linux_architecture() {
            LinuxArchitecture::X86_64 => "thing-linux-aarch64.appimage",
            LinuxArchitecture::AArch64 => "thing-linux-x86_64.appimage",
            LinuxArchitecture::Unknown => "thing-linux.appimage",
        };
        assert_eq!(
            asset_targets_different_linux_architecture(foreign, current_linux_architecture()),
            current_linux_architecture() != LinuxArchitecture::Unknown
        );
    }

    #[test]
    fn appimage_header_requires_64_bit_little_endian_current_machine() {
        let bytes = appimage_bytes();
        assert!(has_expected_appimage_header(&bytes));
        let mut wrong_class = bytes.clone();
        wrong_class[4] = 1;
        assert!(!has_expected_appimage_header(&wrong_class));
        if current_elf_machine() != 0 {
            let mut wrong_machine = bytes;
            wrong_machine[18] ^= 1;
            assert!(!has_expected_appimage_header(&wrong_machine));
        }
    }

    #[test]
    fn bundle_candidate_order_matches_cpp_release_names() {
        let names = appimage_names_for_current_architecture(
            "Spencer-Macro-Utilities-V9.8.7-Linux-x86_64.zip",
            "9.8.7",
        );
        if current_linux_architecture() != LinuxArchitecture::Unknown {
            assert_eq!(
                names.first().map(String::as_str),
                Some("Spencer-Macro-Utilities-V9.8.7-Linux-x86_64.AppImage")
            );
            assert!(names.iter().any(|name| name.contains("V9.8.7-Linux-")));
        }
    }

    #[test]
    fn exact_root_appimage_is_extracted_and_nested_entry_is_rejected() {
        if current_linux_architecture() == LinuxArchitecture::Unknown {
            return;
        }
        let temporary = TestDirectory::new("zip");
        let candidates = appimage_names_for_current_architecture("bundle-linux.zip", "9.8.7");
        let candidate = candidates[1].clone();
        let package = temporary.path.join("package.zip");
        fs::write(&package, zip_with_entry(&candidate, &appimage_bytes()))
            .expect("package should be written");
        let output = temporary.path.join("staged.AppImage");
        extract_linux_appimage_from_package(&package, "bundle-linux.zip", "9.8.7", &output)
            .expect("root AppImage should extract");
        assert_eq!(fs::read(&output).expect("staged bytes"), appimage_bytes());

        let nested = temporary.path.join("nested.zip");
        fs::write(&nested, zip_with_entry(&format!("nested/{candidate}"), &appimage_bytes()))
            .expect("nested package should be written");
        let error =
            extract_linux_appimage_from_package(&nested, "bundle-linux.zip", "9.8.7", &output)
                .expect_err("nested AppImage must not match root candidate");
        assert_eq!(
            error.to_string(),
            "The Linux update bundle did not contain the expected root-level AppImage for this architecture."
        );
    }

    #[test]
    fn replaceability_probe_uses_only_supplied_temp_directory() {
        let temporary = TestDirectory::new("replaceable");
        let appimage = temporary.path.join("SMU.AppImage");
        fs::write(&appimage, appimage_bytes()).expect("AppImage fixture should be written");
        let uid = effective_uid().expect("Linux effective uid should be readable");
        assert_eq!(validate_replaceable_appimage(&appimage, uid), Ok(appimage));
    }

    #[test]
    fn downloaded_asset_verifies_size_and_sha256() {
        let temporary = TestDirectory::new("hash");
        let path = temporary.path.join("asset.bin");
        fs::write(&path, b"abc").expect("asset fixture should be written");
        let sha = format!("{:x}", Sha256::digest(b"abc"));
        let asset = ReleaseAsset {
            name: "test.AppImage".to_owned(),
            download_url: format!("{TRUSTED_RELEASE_ASSET_PREFIX}V9.8.7/test.AppImage"),
            size_bytes: 3,
            sha256: sha,
        };
        verify_downloaded_asset(&asset, &path).expect("matching asset should verify");
    }

    #[test]
    fn updater_script_contains_wait_swap_relaunch_and_rollback_guards() {
        let script = build_linux_updater_script();
        for expected in [
            "while kill -0 \"$OLD_PID\"",
            "BACKUP_APPIMAGE=\"${OLD_APPIMAGE}.old.${OLD_PID}.$$\"",
            "mv -- \"$OLD_APPIMAGE\" \"$BACKUP_APPIMAGE\"",
            "mv -- \"$NEW_APPIMAGE\" \"$OLD_APPIMAGE\"",
            "Updated AppImage exited during startup; restoring the previous version",
            "rm -f -- \"$BACKUP_APPIMAGE\"",
        ] {
            assert!(script.contains(expected), "missing script guard: {expected}");
        }
    }

    #[test]
    fn path_within_requires_a_strict_descendant() {
        assert!(path_is_within(Path::new("/a/b/c"), Path::new("/a/b")));
        assert!(!path_is_within(Path::new("/a/b"), Path::new("/a/b")));
        assert!(!path_is_within(Path::new("/a/bc"), Path::new("/a/b")));
    }
}
