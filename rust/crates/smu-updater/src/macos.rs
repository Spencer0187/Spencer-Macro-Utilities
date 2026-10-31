//! macOS updater install-mode eligibility and release-signature checks.
//!
//! This module deliberately stops before replacement. It establishes the same preconditions used by the
//! legacy updater so update checks can distinguish a valid installed `.app` from DMG/manual-only launches.

use crate::macos_bundle::validate_macos_bundle_archive;
use crate::macos_replace::PreparedMacosReplacement;
use crate::{ReleaseAsset, read_verified_update_asset};
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

fn validated_macos_bundle_path(bundle: &Path) -> Result<PathBuf, MacosUpdateError> {
    let metadata = fs::symlink_metadata(bundle).map_err(|error| {
        MacosUpdateError::new(format!("Could not inspect the SMU app bundle: {error}"))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(MacosUpdateError::new(
            "macOS auto-update requires a regular .app bundle directory.",
        ));
    }
    let canonical = fs::canonicalize(bundle).map_err(|error| {
        MacosUpdateError::new(format!("Could not canonicalize the SMU app bundle: {error}"))
    })?;
    if canonical.extension().and_then(|value| value.to_str()) != Some("app") {
        return Err(MacosUpdateError::new(
            "macOS auto-update requires running from a .app bundle.",
        ));
    }
    Ok(canonical)
}

fn command_succeeded(program: &str, arguments: &[&str], path: &Path) -> bool {
    Command::new(program).args(arguments).arg(path).status().is_ok_and(|status| status.success())
}

fn release_leaf_certificate(bundle: &Path) -> Result<Vec<u8>, MacosUpdateError> {
    if !command_succeeded("/usr/bin/codesign", &["--verify", "--deep", "--strict"], bundle) {
        return Err(MacosUpdateError::new(
            "Automatic updates require an SMU app with a valid release signature.",
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
    let certificate = if extracted {
        fs::read(&leaf_certificate).ok().filter(|bytes| !bytes.is_empty())
    } else {
        None
    };
    let _ = fs::remove_dir_all(&signer_dir);

    certificate.ok_or_else(|| {
        MacosUpdateError::new(
            "Automatic updates require the official certificate-signed SMU app. This build must be updated manually.",
        )
    })
}

fn certificate_backed_release_signature(bundle: &Path) -> Result<(), MacosUpdateError> {
    release_leaf_certificate(bundle).map(|_| ())
}

pub fn path_has_extended_acl(parent: &Path) -> Result<bool, MacosUpdateError> {
    let output = Command::new("/bin/ls")
        .env("LC_ALL", "C")
        .args(["-l", "-d", "-e"])
        .arg(parent)
        .output()
        .map_err(|error| {
            MacosUpdateError::new(format!(
                "Could not inspect the macOS app containing-folder ACL: {error}"
            ))
        })?;
    if !output.status.success() {
        return Err(MacosUpdateError::new(
            "Could not inspect the macOS app containing-folder ACL.",
        ));
    }
    let listing = String::from_utf8_lossy(&output.stdout);
    Ok(listing.lines().skip(1).any(|line| {
        line.trim_start().split_once(':').is_some_and(|(index, _)| {
            !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())
        })
    }))
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
    if effective_user != parent_metadata.uid() {
        return Err(MacosUpdateError::new(
            "Automatic updates require the app's containing folder to be owned by the current user.",
        ));
    }
    if parent_metadata.mode() & 0o022 != 0 {
        return Err(MacosUpdateError::new(
            "Automatic updates are unavailable from a group- or world-writable containing folder.",
        ));
    }
    if path_has_extended_acl(parent)? {
        return Err(MacosUpdateError::new(
            "Automatic updates are unavailable from a containing folder with an extended ACL.",
        ));
    }
    if parent_metadata.mode() & u32::from(libc::S_ISVTX) != 0
        && effective_user != 0
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

/// Return the canonical installed `.app` bundle when the supplied bundle satisfies the legacy macOS
/// self-replacement preconditions.
///
/// # Errors
///
/// Returns the user-facing reason that this bundle must use manual installation instead.
pub fn macos_update_bundle_if_replaceable_at(bundle: &Path) -> Result<PathBuf, MacosUpdateError> {
    let bundle = validated_macos_bundle_path(bundle)?;
    if mounted_dmg_path(&bundle) {
        return Err(MacosUpdateError::new(
            "Automatic installation is unavailable while SMU is running from a mounted DMG. Drag the app to Applications and launch it there first.",
        ));
    }
    certificate_backed_release_signature(&bundle)?;
    replacement_parent_is_safe(&bundle)?;
    Ok(bundle)
}

/// Return the canonical installed `.app` bundle when the current launch satisfies the legacy macOS
/// self-replacement preconditions.
///
/// # Errors
///
/// Returns the user-facing reason that this launch must use manual installation instead.
pub fn macos_update_bundle_if_replaceable() -> Result<PathBuf, MacosUpdateError> {
    macos_update_bundle_if_replaceable_at(&current_app_bundle_path()?)
}

#[derive(Debug)]
pub struct StagedMacosBundle {
    work_dir: PathBuf,
    app_path: PathBuf,
}

impl StagedMacosBundle {
    #[must_use]
    pub fn app_path(&self) -> &Path {
        &self.app_path
    }
}

impl Drop for StagedMacosBundle {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.work_dir);
    }
}

/// Copy an already-authenticated staged `.app` into a private same-filesystem swap directory beside the
/// installed bundle and revalidate its signer after that copy.
///
/// This remains dormant: it prepares paths for the replacement transaction but does not rename the installed
/// application or launch anything.
///
/// # Errors
///
/// Returns when install eligibility changes, the same-directory copy fails, the copied bundle is substituted,
/// or its release signer no longer matches the installed app.
pub fn prepare_macos_replacement_for_bundle(
    staged: &StagedMacosBundle,
    installed_bundle: &Path,
) -> Result<PreparedMacosReplacement, MacosUpdateError> {
    let current_app = macos_update_bundle_if_replaceable_at(installed_bundle)?;
    let parent = current_app.parent().ok_or_else(|| {
        MacosUpdateError::new("Could not resolve the installed macOS app containing directory.")
    })?;
    let swap_dir = unique_directory(parent, ".smu-update-swap")?;
    let preparation = (|| {
        let new_app = swap_dir.join("new.app");
        let backup_app = swap_dir.join("old.app");
        let status = Command::new("/usr/bin/ditto")
            .arg(staged.app_path())
            .arg(&new_app)
            .status()
            .map_err(|error| {
            MacosUpdateError::new(format!(
                "Could not copy the validated macOS app beside the installed bundle: {error}"
            ))
        })?;
        if !status.success() {
            return Err(MacosUpdateError::new(
                "Could not copy the validated macOS app beside the installed bundle.",
            ));
        }
        let current_metadata = fs::symlink_metadata(&current_app).map_err(|error| {
            MacosUpdateError::new(format!(
                "Could not inspect the installed macOS app before replacement: {error}"
            ))
        })?;
        let new_metadata = fs::symlink_metadata(&new_app).map_err(|error| {
            MacosUpdateError::new(format!("Could not inspect the prepared macOS app: {error}"))
        })?;
        if new_metadata.file_type().is_symlink() || !new_metadata.is_dir() {
            return Err(MacosUpdateError::new(
                "Prepared macOS update is not a regular app bundle directory.",
            ));
        }
        // Snapshot identities before signature verification; PreparedMacosReplacement::from_paths rechecks
        // these exact objects after codesign completes, closing substitution between verification and pinning.
        validate_staged_bundle_signer(&current_app, &new_app)?;
        PreparedMacosReplacement::from_paths(
            &swap_dir,
            &current_app,
            &new_app,
            &backup_app,
            (current_metadata.dev(), current_metadata.ino()),
            (new_metadata.dev(), new_metadata.ino()),
        )
        .map_err(|error| MacosUpdateError::new(error.to_string()))
    })();
    if preparation.is_err() {
        let _ = fs::remove_dir_all(&swap_dir);
    }
    preparation
}

/// Prepare a same-filesystem replacement for the current installed app bundle.
///
/// # Errors
///
/// Returns when current-bundle eligibility, staging, signer validation, or descriptor pinning fails.
pub fn prepare_macos_replacement(
    staged: &StagedMacosBundle,
) -> Result<PreparedMacosReplacement, MacosUpdateError> {
    prepare_macos_replacement_for_bundle(staged, &current_app_bundle_path()?)
}

/// Require two macOS code objects to carry the exact same certificate-backed leaf release signer.
///
/// # Errors
///
/// Returns when either code object fails strict code-signature verification, lacks a leaf certificate, or the
/// extracted leaf certificates differ byte-for-byte.
pub fn require_matching_macos_release_signer(
    trusted_code: &Path,
    candidate_code: &Path,
) -> Result<(), MacosUpdateError> {
    let trusted_certificate = release_leaf_certificate(trusted_code)?;
    let candidate_certificate = release_leaf_certificate(candidate_code).map_err(|_| {
        MacosUpdateError::new("Candidate macOS code failed release-signature validation.")
    })?;
    if trusted_certificate != candidate_certificate {
        return Err(MacosUpdateError::new(
            "Candidate macOS code was not signed by the same release certificate.",
        ));
    }
    Ok(())
}

fn validate_staged_bundle_signer(
    installed_bundle: &Path,
    staged_bundle: &Path,
) -> Result<(), MacosUpdateError> {
    require_matching_macos_release_signer(installed_bundle, staged_bundle).map_err(|error| {
        MacosUpdateError::new(format!("Downloaded macOS app failed signer validation: {error}"))
    })
}

/// Stage one manifest-authenticated macOS ZIP into a private temporary directory and require a single,
/// valid `.app` signed by the same leaf release certificate as the installed bundle.
///
/// This function is intentionally dormant: it prepares and validates a bundle but never mutates the installed
/// application or launches a replacement helper.
///
/// # Errors
///
/// Returns an error when the asset contract, ZIP layout, `ditto` extraction, staged bundle type, or signature
/// identity check fails.
pub fn stage_verified_macos_update_asset_for_bundle(
    asset: &ReleaseAsset,
    downloaded_path: &Path,
    installed_bundle: &Path,
) -> Result<StagedMacosBundle, MacosUpdateError> {
    if !asset
        .name
        .rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("zip"))
    {
        return Err(MacosUpdateError::new(
            "macOS auto-update only supports ZIP release assets containing a .app bundle.",
        ));
    }
    let installed_bundle = macos_update_bundle_if_replaceable_at(installed_bundle)?;
    let authenticated_bytes = read_verified_update_asset(asset, downloaded_path)
        .map_err(|error| MacosUpdateError::new(error.to_string()))?;
    let app_name = validate_macos_bundle_archive(&authenticated_bytes)
        .map_err(|error| MacosUpdateError::new(error.to_string()))?;

    let work_dir = unique_directory(Path::new("/tmp"), "smu-macos-updater")?;
    let staging = (|| {
        let zip_path = work_dir.join("update.zip");
        fs::write(&zip_path, &authenticated_bytes).map_err(|error| {
            MacosUpdateError::new(format!(
                "Could not write authenticated macOS update ZIP: {error}"
            ))
        })?;
        let extracted = unique_directory(&work_dir, "extracted")?;
        let status = Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(&zip_path)
            .arg(&extracted)
            .status()
            .map_err(|error| {
                MacosUpdateError::new(format!("Could not launch macOS ZIP extraction: {error}"))
            })?;
        if !status.success() {
            return Err(MacosUpdateError::new("Downloaded macOS update could not be extracted."));
        }
        let _ = fs::remove_file(&zip_path);
        let app_path = extracted.join(app_name);
        let metadata = fs::symlink_metadata(&app_path).map_err(|error| {
            MacosUpdateError::new(format!("Downloaded macOS app bundle was missing: {error}"))
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(MacosUpdateError::new(
                "Downloaded macOS update did not contain a regular app bundle directory.",
            ));
        }
        validate_staged_bundle_signer(&installed_bundle, &app_path)?;
        Ok(StagedMacosBundle { work_dir: work_dir.clone(), app_path })
    })();
    if staging.is_err() {
        let _ = fs::remove_dir_all(&work_dir);
    }
    staging
}

/// Stage one verified macOS update asset against the current installed app bundle.
///
/// # Errors
///
/// Returns when current-bundle eligibility, asset verification, extraction, or signer validation fails.
pub fn stage_verified_macos_update_asset(
    asset: &ReleaseAsset,
    downloaded_path: &Path,
) -> Result<StagedMacosBundle, MacosUpdateError> {
    stage_verified_macos_update_asset_for_bundle(
        asset,
        downloaded_path,
        &current_app_bundle_path()?,
    )
}

#[must_use]
pub fn macos_auto_apply_supported() -> bool {
    macos_update_bundle_if_replaceable().is_ok()
}

#[must_use]
pub fn macos_auto_apply_unavailable_reason() -> Option<String> {
    macos_update_bundle_if_replaceable().err().map(|error| error.to_string())
}
