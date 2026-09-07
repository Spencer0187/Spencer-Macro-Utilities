//! macOS updater install-mode eligibility and release-signature checks.
//!
//! This module deliberately stops before replacement. It establishes the same preconditions used by the
//! legacy updater so update checks can distinguish a valid installed `.app` from DMG/manual-only launches.

use std::fmt;
use std::fs;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const TEMP_ATTEMPTS: usize = 128;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MacosUpdateError(String);

impl MacosUpdateError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for MacosUpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for MacosUpdateError {}

fn unique_directory(parent: &Path, prefix: &str) -> Result<PathBuf, MacosUpdateError> {
    let epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    for _ in 0..TEMP_ATTEMPTS {
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!("{prefix}-{}-{epoch}-{counter}", std::process::id()));
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(MacosUpdateError::new(format!(
                    "Could not create a macOS update staging probe: {error}"
                )));
            }
        }
    }
    Err(MacosUpdateError::new("Could not allocate a unique macOS update staging probe."))
}

fn current_app_bundle_path() -> Result<PathBuf, MacosUpdateError> {
    let executable = std::env::current_exe().map_err(|error| {
        MacosUpdateError::new(format!("Could not resolve the current macOS executable: {error}"))
    })?;
    let executable = fs::canonicalize(executable).map_err(|error| {
        MacosUpdateError::new(format!(
            "Could not canonicalize the current macOS executable: {error}"
        ))
    })?;
    let contents = executable.parent().and_then(Path::parent).ok_or_else(|| {
        MacosUpdateError::new("macOS auto-update requires running from a .app bundle.")
    })?;
    let bundle = contents.parent().ok_or_else(|| {
        MacosUpdateError::new("macOS auto-update requires running from a .app bundle.")
    })?;
    if bundle.extension().and_then(|value| value.to_str()) != Some("app") {
        return Err(MacosUpdateError::new(
            "macOS auto-update requires running from a .app bundle.",
        ));
    }
    let metadata = fs::symlink_metadata(bundle).map_err(|error| {
        MacosUpdateError::new(format!("Could not inspect the current SMU app bundle: {error}"))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(MacosUpdateError::new("Could not resolve the current SMU app bundle on disk."));
    }
    Ok(bundle.to_path_buf())
}

fn mounted_dmg_path(path: &Path) -> bool {
    path == Path::new("/Volumes") || path.starts_with("/Volumes")
}

fn command_succeeded(program: &str, arguments: &[&str], path: &Path) -> bool {
    Command::new(program).args(arguments).arg(path).status().is_ok_and(|status| status.success())
}

fn certificate_backed_release_signature(bundle: &Path) -> Result<(), MacosUpdateError> {
    if !command_succeeded("/usr/bin/codesign", &["--verify", "--deep", "--strict"], bundle) {
        return Err(MacosUpdateError::new(
            "Automatic updates require an installed SMU app with a valid release signature.",
        ));
    }

    let signer_dir = unique_directory(Path::new("/tmp"), "smu-macos-signer")?;
    let signer_prefix = signer_dir.join("signer-");
    let extract_option = format!("--extract-certificates={}", signer_prefix.display());
    let extracted = Command::new("/usr/bin/codesign")
        .args(["-d", extract_option.as_str()])
        .arg(bundle)
        .status()
        .is_ok_and(|status| status.success());
    let leaf_certificate = signer_dir.join("signer-0");
    let has_leaf = extracted
        && fs::metadata(&leaf_certificate)
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0);
    let _ = fs::remove_dir_all(&signer_dir);

    if !has_leaf {
        return Err(MacosUpdateError::new(
            "Automatic updates require the official certificate-signed SMU app. This build must be updated manually.",
        ));
    }
    Ok(())
}

fn replacement_parent_is_safe(bundle: &Path) -> Result<(), MacosUpdateError> {
    let parent = bundle.parent().ok_or_else(|| {
        MacosUpdateError::new("Could not resolve the current SMU app containing folder.")
    })?;
    let parent_metadata = fs::metadata(parent).map_err(|error| {
        MacosUpdateError::new(format!("Could not inspect the current SMU app location: {error}"))
    })?;
    let bundle_metadata = fs::symlink_metadata(bundle).map_err(|error| {
        MacosUpdateError::new(format!("Could not inspect the current SMU app location: {error}"))
    })?;
    let effective_user = unsafe { libc::geteuid() };
    if parent_metadata.mode() & libc::S_ISVTX != 0
        && effective_user != 0
        && effective_user != parent_metadata.uid()
        && effective_user != bundle_metadata.uid()
    {
        return Err(MacosUpdateError::new(
            "Cannot replace this app because it is owned by another user in a protected folder.",
        ));
    }

    let probe = unique_directory(parent, ".smu-update-write-test").map_err(|_| {
        MacosUpdateError::new(
            "SMU cannot update itself because its containing folder is not writable.",
        )
    })?;
    fs::remove_dir(&probe).map_err(|error| {
        MacosUpdateError::new(format!(
            "Could not clean up the macOS update writeability probe: {error}"
        ))
    })
}

/// Return the canonical installed `.app` bundle when the current launch satisfies the legacy macOS
/// self-replacement preconditions.
///
/// # Errors
///
/// Returns the user-facing reason that this launch must use manual installation instead.
pub fn macos_update_bundle_if_replaceable() -> Result<PathBuf, MacosUpdateError> {
    let bundle = current_app_bundle_path()?;
    if mounted_dmg_path(&bundle) {
        return Err(MacosUpdateError::new(
            "Automatic installation is unavailable while SMU is running from a mounted DMG. Drag the app to Applications and launch it there first.",
        ));
    }
    certificate_backed_release_signature(&bundle)?;
    replacement_parent_is_safe(&bundle)?;
    Ok(bundle)
}

#[must_use]
pub fn macos_auto_apply_supported() -> bool {
    macos_update_bundle_if_replaceable().is_ok()
}

#[must_use]
pub fn macos_auto_apply_unavailable_reason() -> Option<String> {
    macos_update_bundle_if_replaceable().err().map(|error| error.to_string())
}
