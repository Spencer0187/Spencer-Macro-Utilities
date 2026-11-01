use crate::MAXIMUM_UPDATE_BYTES;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{Cursor, Read as _};
use std::path::{Path, PathBuf};
use zip::ZipArchive;

pub const WINDOWS_BUNDLE_ROOT: &str = "Spencer-Macro-Utilities";
/// Only SMU-owned executable images may receive our `SignPath` Foundation certificate.
///
/// The helper checks these against the installed app's release signer before parent exit.
pub const WINDOWS_RELEASE_SIGNER_FILES: &[&str] =
    &["Spencer-Macro-Utilities.exe", "SMU-Update-Helper.exe"];
/// Unsigned third-party DLLs may not be re-signed with SMU's Foundation certificate.
///
/// The strict interim trust policy allows automatic replacement only if these DLLs have
/// the **same SHA-256 digest** as their already-installed, trusted counterparts. New
/// upstream dependency versions therefore require a separately authorized manual update.
pub const WINDOWS_UNCHANGED_UPSTREAM_LIBRARY_FILES: &[&str] = &["SDL3.dll", "SMCWinDivert.dll"];
/// PE files whose upstream signer must remain continuous across an automatic update.
///
/// `WinDivert`'s kernel driver keeps its upstream kernel-compatible signature rather than being re-signed with the
/// SMU release certificate. Automatic updates therefore require the staged driver to match the installed driver's
/// Authenticode signer.
pub const WINDOWS_UPSTREAM_SIGNER_CONTINUITY_FILES: &[&str] =
    &["SMCWinDivert64.sys", "WinDivert64.sys"];
const MAXIMUM_BUNDLE_ENTRIES: usize = 4096;
const REQUIRED_FILES: &[&str] = &[
    "Spencer-Macro-Utilities.exe",
    "SMU-Update-Helper.exe",
    "SDL3.dll",
    "SMCWinDivert.dll",
    "SMCWinDivert64.sys",
    "WinDivert64.sys",
    "assets/LSANS.TTF",
];
const ALLOWED_TOP_LEVEL_FILES: &[&str] = &[
    "Spencer-Macro-Utilities.exe",
    "SMU-Update-Helper.exe",
    "SDL3.dll",
    "SMCWinDivert.dll",
    "SMCWinDivert64.sys",
    "WinDivert64.sys",
    "README.md",
    "LICENSE",
    "PRIVACY.md",
    "THIRD_PARTY_NOTICES.md",
];
const ALLOWED_ASSET_EXTENSIONS: &[&str] = &["bmp", "icns", "jpeg", "jpg", "png", "ttf"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsBundleError(String);

impl WindowsBundleError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for WindowsBundleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for WindowsBundleError {}

fn normalized_relative_name(name: &str) -> Result<Option<String>, WindowsBundleError> {
    if name.contains(['\\', ':']) || name.starts_with('/') {
        return Err(WindowsBundleError::new("Windows update bundle contains an unsafe path."));
    }
    let parts = name.split('/').collect::<Vec<_>>();
    if parts.iter().any(|part| matches!(*part, "." | "..")) {
        return Err(WindowsBundleError::new("Windows update bundle contains an unsafe path."));
    }
    let Some((root, relative)) = parts.split_first() else {
        return Err(WindowsBundleError::new("Windows update bundle contains an empty path."));
    };
    if *root != WINDOWS_BUNDLE_ROOT {
        return Err(WindowsBundleError::new(
            "Windows update bundle contains files outside the expected package root.",
        ));
    }
    let relative =
        relative.iter().copied().filter(|part| !part.is_empty()).collect::<Vec<_>>().join("/");
    if relative.is_empty() {
        return Ok(None);
    }
    Ok(Some(relative))
}

fn entry_is_regular_file(mode: Option<u32>) -> bool {
    mode.is_none_or(|value| {
        let kind = value & 0o170_000;
        kind == 0 || kind == 0o100_000
    })
}

fn bundle_path_allowed(relative: &str) -> bool {
    if ALLOWED_TOP_LEVEL_FILES.contains(&relative) {
        return true;
    }
    if let Some(asset) = relative.strip_prefix("assets/") {
        let extension = Path::new(asset)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        return !asset.is_empty() && ALLOWED_ASSET_EXTENSIONS.contains(&extension.as_str());
    }
    relative
        .strip_prefix("licenses/")
        .is_some_and(|name| !name.is_empty() && name.to_ascii_lowercase().ends_with(".txt"))
}

fn validate_bundle_archive(bytes: &[u8]) -> Result<BTreeSet<String>, WindowsBundleError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| {
        WindowsBundleError::new("Downloaded Windows update was not a valid ZIP archive.")
    })?;
    if archive.is_empty() || archive.len() > MAXIMUM_BUNDLE_ENTRIES {
        return Err(WindowsBundleError::new(
            "Windows update bundle contains an invalid number of entries.",
        ));
    }

    let mut total_size = 0_u64;
    let mut files = BTreeSet::new();
    let mut driver_hashes = BTreeMap::<String, [u8; 32]>::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|_| {
            WindowsBundleError::new("Windows update bundle contains an unreadable ZIP entry.")
        })?;
        let relative = normalized_relative_name(entry.name())?;
        if entry.is_dir() {
            continue;
        }
        if entry.encrypted() || !entry_is_regular_file(entry.unix_mode()) {
            return Err(WindowsBundleError::new(
                "Windows update bundle contains an unsupported ZIP entry.",
            ));
        }
        let Some(relative) = relative else {
            return Err(WindowsBundleError::new(
                "Windows update bundle contains an invalid root entry.",
            ));
        };
        if !bundle_path_allowed(&relative) {
            return Err(WindowsBundleError::new(format!(
                "Windows update bundle contains an unexpected file: {relative}"
            )));
        }
        if !files.insert(relative.clone()) {
            return Err(WindowsBundleError::new(
                "Windows update bundle contains a duplicate file path.",
            ));
        }
        if matches!(relative.as_str(), "SMCWinDivert64.sys" | "WinDivert64.sys") {
            let mut hasher = Sha256::new();
            let mut chunk = vec![0_u8; 64 * 1024].into_boxed_slice();
            loop {
                let count = entry.read(&mut chunk).map_err(|error| {
                    WindowsBundleError::new(format!(
                        "Could not read the signed WinDivert driver: {error}"
                    ))
                })?;
                if count == 0 {
                    break;
                }
                hasher.update(&chunk[..count]);
            }
            driver_hashes.insert(relative, hasher.finalize().into());
        }
        total_size = total_size.checked_add(entry.size()).ok_or_else(|| {
            WindowsBundleError::new("Windows update bundle expanded size overflowed.")
        })?;
        if total_size > MAXIMUM_UPDATE_BYTES {
            return Err(WindowsBundleError::new(
                "Windows update bundle exceeds the 512 MiB expanded-size limit.",
            ));
        }
    }
    for required in REQUIRED_FILES {
        if !files.contains(*required) {
            return Err(WindowsBundleError::new(format!(
                "Windows update bundle is missing required runtime file: {required}"
            )));
        }
    }
    if driver_hashes.get("SMCWinDivert64.sys") != driver_hashes.get("WinDivert64.sys") {
        return Err(WindowsBundleError::new(
            "The two upstream-signed WinDivert driver filenames must have byte-identical contents.",
        ));
    }
    Ok(files)
}

/// Calculate one SHA-256 digest per validated regular file in an authenticated Windows bundle.
///
/// # Errors
///
/// Returns an error when the archive is malformed, violates the Windows bundle contract, or an entry
/// cannot be read completely.
pub fn windows_bundle_file_sha256(
    bytes: &[u8],
) -> Result<BTreeMap<String, [u8; 32]>, WindowsBundleError> {
    let expected_files = validate_bundle_archive(bytes)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| {
        WindowsBundleError::new("Downloaded Windows update was not a valid ZIP archive.")
    })?;
    let mut digests = BTreeMap::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|_| {
            WindowsBundleError::new("Windows update bundle contains an unreadable ZIP entry.")
        })?;
        if entry.is_dir() {
            continue;
        }
        let Some(relative) = normalized_relative_name(entry.name())? else {
            continue;
        };
        if !expected_files.contains(&relative) || digests.contains_key(&relative) {
            return Err(WindowsBundleError::new(
                "Windows update bundle changed during digest calculation.",
            ));
        }
        let mut entry_hasher = Sha256::new();
        loop {
            let count = entry.read(&mut buffer).map_err(|error| {
                WindowsBundleError::new(format!(
                    "Could not read a Windows update bundle entry while hashing: {error}"
                ))
            })?;
            if count == 0 {
                break;
            }
            entry_hasher.update(&buffer[..count]);
        }
        let digest: [u8; 32] = entry_hasher.finalize().into();
        digests.insert(relative, digest);
    }
    if digests.len() != expected_files.len()
        || !expected_files.iter().all(|relative| digests.contains_key(relative))
    {
        return Err(WindowsBundleError::new(
            "Windows update bundle digest set did not match the validated file set.",
        ));
    }
    Ok(digests)
}

/// Reject replacements of upstream DLLs until a signed dependency-update policy exists.
///
/// # Errors
///
/// Returns an error for missing, empty, symbolic-link, overlarge, unreadable or changed
/// external libraries. Both files are hashed in bounded-memory chunks. The Windows
/// helper additionally validates their paths for NTFS reparse points before calling.
pub fn require_unchanged_windows_upstream_library(
    installed: &Path,
    staged: &Path,
) -> Result<(), WindowsBundleError> {
    fn sha256_regular_file(path: &Path) -> Result<[u8; 32], WindowsBundleError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            WindowsBundleError::new(format!(
                "Could not inspect an external Windows runtime library: {error}"
            ))
        })?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() == 0
            || metadata.len() > MAXIMUM_UPDATE_BYTES
        {
            return Err(WindowsBundleError::new(
                "External Windows runtime library must be a non-empty regular file within the update-size limit.",
            ));
        }
        let mut file = File::open(path).map_err(|error| {
            WindowsBundleError::new(format!(
                "Could not open an external Windows runtime library: {error}"
            ))
        })?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 16 * 1024];
        let mut total_bytes = 0_u64;
        loop {
            let count = file.read(&mut buffer).map_err(|error| {
                WindowsBundleError::new(format!(
                    "Could not hash an external Windows runtime library: {error}"
                ))
            })?;
            if count == 0 {
                break;
            }
            total_bytes =
                total_bytes.checked_add(u64::try_from(count).unwrap_or(u64::MAX)).ok_or_else(
                    || WindowsBundleError::new("External Windows runtime library size overflowed."),
                )?;
            if total_bytes > MAXIMUM_UPDATE_BYTES {
                return Err(WindowsBundleError::new(
                    "External Windows runtime library exceeds the update-size limit.",
                ));
            }
            hasher.update(&buffer[..count]);
        }
        if total_bytes != metadata.len() {
            return Err(WindowsBundleError::new(
                "External Windows runtime library changed while hashing.",
            ));
        }
        Ok(hasher.finalize().into())
    }

    let installed_digest = sha256_regular_file(installed)?;
    let staged_digest = sha256_regular_file(staged)?;
    if installed_digest != staged_digest {
        return Err(WindowsBundleError::new(
            "External Windows runtime DLL differs from the installed version; a manual update is required.",
        ));
    }
    Ok(())
}

pub fn destination_path(root: &Path, relative: &str) -> PathBuf {
    relative.split('/').fold(root.to_path_buf(), |path, part| path.join(part))
}

fn tree_relative_name(root: &Path, path: &Path) -> Result<String, WindowsBundleError> {
    let relative = path.strip_prefix(root).map_err(|_| {
        WindowsBundleError::new("Windows update staging tree escaped the expected package root.")
    })?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let value = component.as_os_str().to_str().ok_or_else(|| {
            WindowsBundleError::new("Windows update staging tree contains a non-UTF-8 path.")
        })?;
        if value.is_empty() || matches!(value, "." | "..") {
            return Err(WindowsBundleError::new(
                "Windows update staging tree contains an unsafe path.",
            ));
        }
        parts.push(value);
    }
    Ok(parts.join("/"))
}

pub fn validate_windows_bundle_tree(
    package_root: &Path,
) -> Result<Vec<String>, WindowsBundleError> {
    if !package_root.is_dir()
        || package_root.file_name().and_then(|value| value.to_str()) != Some(WINDOWS_BUNDLE_ROOT)
    {
        return Err(WindowsBundleError::new(
            "Windows update staging tree does not use the expected package root.",
        ));
    }
    let mut pending = vec![package_root.to_path_buf()];
    let mut files = BTreeSet::new();
    let mut total_size = 0_u64;
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory).map_err(|error| {
            WindowsBundleError::new(format!(
                "Could not inspect the Windows update staging tree: {error}"
            ))
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                WindowsBundleError::new(format!(
                    "Could not inspect a Windows update staging entry: {error}"
                ))
            })?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                WindowsBundleError::new(format!(
                    "Could not inspect Windows update staging metadata: {error}"
                ))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(WindowsBundleError::new(
                    "Windows update staging tree contains a symbolic link.",
                ));
            }
            if metadata.is_dir() {
                pending.push(path);
                continue;
            }
            if !metadata.is_file() {
                return Err(WindowsBundleError::new(
                    "Windows update staging tree contains a non-regular entry.",
                ));
            }
            let relative = tree_relative_name(package_root, &path)?;
            if !bundle_path_allowed(&relative) {
                return Err(WindowsBundleError::new(format!(
                    "Windows update staging tree contains an unexpected file: {relative}"
                )));
            }
            if !files.insert(relative) || files.len() > MAXIMUM_BUNDLE_ENTRIES {
                return Err(WindowsBundleError::new(
                    "Windows update staging tree contains too many or duplicate files.",
                ));
            }
            total_size = total_size.checked_add(metadata.len()).ok_or_else(|| {
                WindowsBundleError::new("Windows update staging tree size overflowed.")
            })?;
            if total_size > MAXIMUM_UPDATE_BYTES {
                return Err(WindowsBundleError::new(
                    "Windows update staging tree exceeds the 512 MiB expanded-size limit.",
                ));
            }
        }
    }
    for required in REQUIRED_FILES {
        let path = destination_path(package_root, required);
        if !files.contains(*required)
            || path.metadata().map(|value| value.len()).unwrap_or_default() == 0
        {
            return Err(WindowsBundleError::new(format!(
                "Windows update staging tree is missing a non-empty required runtime file: {required}"
            )));
        }
    }
    require_unchanged_windows_upstream_library(
        &package_root.join("SMCWinDivert64.sys"),
        &package_root.join("WinDivert64.sys"),
    )
    .map_err(|_| {
        WindowsBundleError::new(
            "The two upstream-signed WinDivert driver filenames must have byte-identical contents.",
        )
    })?;
    Ok(files.into_iter().collect())
}

/// Validate and extract a Rust Windows update bundle into a clean staging directory.
///
/// # Errors
///
/// Returns an error for malformed/unsafe archives, missing runtime files, expanded payloads above the
/// updater limit, or filesystem failures while creating the staging tree.
pub fn extract_windows_bundle(
    bytes: &[u8],
    destination: &Path,
) -> Result<PathBuf, WindowsBundleError> {
    let expected_files = validate_bundle_archive(bytes)?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            WindowsBundleError::new(format!(
                "Could not create the Windows update staging parent directory: {error}"
            ))
        })?;
    }
    fs::create_dir(destination).map_err(|error| {
        WindowsBundleError::new(format!(
            "Could not create a fresh Windows update staging directory: {error}"
        ))
    })?;

    let package_root = destination.join(WINDOWS_BUNDLE_ROOT);
    let result = (|| {
        let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| {
            WindowsBundleError::new("Downloaded Windows update was not a valid ZIP archive.")
        })?;
        let mut extracted = BTreeSet::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|_| {
                WindowsBundleError::new("Windows update bundle contains an unreadable ZIP entry.")
            })?;
            if entry.is_dir() {
                continue;
            }
            let Some(relative) = normalized_relative_name(entry.name())? else {
                continue;
            };
            if !expected_files.contains(&relative) || !extracted.insert(relative.clone()) {
                return Err(WindowsBundleError::new(
                    "Windows update bundle changed during extraction validation.",
                ));
            }
            let output = destination_path(&package_root, &relative);
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    WindowsBundleError::new(format!(
                        "Could not create a Windows update staging directory: {error}"
                    ))
                })?;
            }
            let mut file = File::create(&output).map_err(|error| {
                WindowsBundleError::new(format!(
                    "Could not create a staged Windows update file: {error}"
                ))
            })?;
            std::io::copy(&mut entry, &mut file).map_err(|error| {
                WindowsBundleError::new(format!("Could not extract a Windows update file: {error}"))
            })?;
        }
        if extracted != expected_files {
            return Err(WindowsBundleError::new(
                "Windows update bundle extraction did not reproduce the validated file set.",
            ));
        }
        for required in REQUIRED_FILES {
            let path = destination_path(&package_root, required);
            let size = path.metadata().map(|metadata| metadata.len()).unwrap_or_default();
            if size == 0 {
                return Err(WindowsBundleError::new(format!(
                    "Windows update bundle contains an empty required runtime file: {required}"
                )));
            }
        }
        Ok(package_root.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("smu-windows-bundle-{label}-{}-{counter}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("temporary directory should be created");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn bundle(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();
        for (name, bytes) in entries {
            writer.start_file(*name, options).expect("ZIP entry should start");
            writer.write_all(bytes).expect("ZIP entry should be written");
        }
        writer.finish().expect("ZIP should finish").into_inner()
    }

    fn valid_entries() -> Vec<(&'static str, &'static [u8])> {
        vec![
            ("Spencer-Macro-Utilities/Spencer-Macro-Utilities.exe", b"exe"),
            ("Spencer-Macro-Utilities/SMU-Update-Helper.exe", b"helper"),
            ("Spencer-Macro-Utilities/SDL3.dll", b"sdl"),
            ("Spencer-Macro-Utilities/SMCWinDivert.dll", b"dll"),
            ("Spencer-Macro-Utilities/SMCWinDivert64.sys", b"driver"),
            ("Spencer-Macro-Utilities/WinDivert64.sys", b"driver"),
            ("Spencer-Macro-Utilities/assets/LSANS.TTF", b"font"),
            ("Spencer-Macro-Utilities/assets/macro_tutorials/wallhop.jpg", b"image"),
            ("Spencer-Macro-Utilities/LICENSE", b"license"),
        ]
    }

    #[test]
    fn executable_trust_policy_covers_every_required_pe_runtime_once() {
        let release = WINDOWS_RELEASE_SIGNER_FILES.iter().copied().collect::<BTreeSet<_>>();
        let upstream =
            WINDOWS_UPSTREAM_SIGNER_CONTINUITY_FILES.iter().copied().collect::<BTreeSet<_>>();
        let unchanged =
            WINDOWS_UNCHANGED_UPSTREAM_LIBRARY_FILES.iter().copied().collect::<BTreeSet<_>>();
        assert!(release.is_disjoint(&upstream));
        assert!(release.is_disjoint(&unchanged));
        assert!(upstream.is_disjoint(&unchanged));
        let covered = release
            .union(&upstream)
            .copied()
            .collect::<BTreeSet<_>>()
            .union(&unchanged)
            .copied()
            .collect::<BTreeSet<_>>();
        let expected = REQUIRED_FILES
            .iter()
            .copied()
            .filter(|relative| !relative.starts_with("assets/"))
            .collect::<BTreeSet<_>>();
        assert_eq!(covered, expected);
    }

    #[test]
    fn unchanged_upstream_library_gate_rejects_missing_or_modified_dlls() {
        let temp = TestDirectory::new("external-dependencies");
        let installed = temp.0.join("installed.dll");
        let staged = temp.0.join("staged.dll");
        fs::write(&installed, b"trusted-upstream-dll").expect("installed DLL fixture");
        fs::write(&staged, b"trusted-upstream-dll").expect("staged DLL fixture");
        assert!(require_unchanged_windows_upstream_library(&installed, &staged).is_ok());

        fs::write(&staged, b"attacker-changed-dll").expect("modified DLL fixture");
        let error = require_unchanged_windows_upstream_library(&installed, &staged)
            .expect_err("changed DLL must be rejected");
        assert!(error.to_string().contains("manual update is required"));

        fs::write(&staged, b"").expect("empty DLL fixture");
        assert!(require_unchanged_windows_upstream_library(&installed, &staged).is_err());
        fs::remove_file(&staged).expect("remove DLL fixture");
        assert!(require_unchanged_windows_upstream_library(&installed, &staged).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn unchanged_upstream_library_gate_rejects_symbolic_links() {
        let temp = TestDirectory::new("external-dependencies-symlink");
        let installed = temp.0.join("installed.dll");
        let staged = temp.0.join("staged.dll");
        fs::write(&installed, b"trusted-upstream-dll").expect("installed DLL fixture");
        std::os::unix::fs::symlink(&installed, &staged).expect("staged symbolic-link fixture");
        assert!(require_unchanged_windows_upstream_library(&installed, &staged).is_err());
    }

    #[test]
    fn authenticated_bundle_hashes_cover_every_validated_file() {
        let bytes = bundle(&valid_entries());
        let hashes = windows_bundle_file_sha256(&bytes).expect("bundle hashes should calculate");
        assert_eq!(hashes.len(), valid_entries().len());
        assert_eq!(hashes.get("SDL3.dll"), Some(&<[u8; 32]>::from(Sha256::digest(b"sdl"))));
        assert_eq!(
            hashes.get("SMU-Update-Helper.exe"),
            Some(&<[u8; 32]>::from(Sha256::digest(b"helper")))
        );
    }

    #[test]
    fn valid_bundle_extracts_exact_rooted_runtime_tree() {
        let temporary = TestDirectory::new("valid");
        let bytes = bundle(&valid_entries());
        let staging = temporary.0.join("stage");
        let root = extract_windows_bundle(&bytes, &staging).expect("bundle should extract");
        assert_eq!(root, staging.join(WINDOWS_BUNDLE_ROOT));
        assert_eq!(fs::read(root.join("SDL3.dll")).expect("SDL file should read"), b"sdl");
        assert_eq!(
            fs::read(root.join("assets/macro_tutorials/wallhop.jpg"))
                .expect("tutorial image should read"),
            b"image"
        );
    }

    #[test]
    fn preexisting_staging_directory_is_rejected_without_modification() {
        let temporary = TestDirectory::new("preexisting");
        let staging = temporary.0.join("stage");
        fs::create_dir(&staging).expect("staging fixture should be created");
        let marker = staging.join("marker.txt");
        fs::write(&marker, b"keep").expect("marker should be written");

        let error = extract_windows_bundle(&bundle(&valid_entries()), &staging)
            .expect_err("preexisting staging directory must be rejected");

        assert!(error.to_string().contains("fresh Windows update staging directory"));
        assert_eq!(fs::read(&marker).expect("marker should remain readable"), b"keep");
        assert!(!staging.join(WINDOWS_BUNDLE_ROOT).exists());
    }

    #[test]
    fn missing_required_runtime_file_is_rejected_before_staging() {
        let temporary = TestDirectory::new("missing");
        let mut entries = valid_entries();
        entries.retain(|(name, _)| !name.ends_with("SMCWinDivert64.sys"));
        let staging = temporary.0.join("stage");
        let error = extract_windows_bundle(&bundle(&entries), &staging)
            .expect_err("missing runtime file must be rejected");
        assert!(error.to_string().contains("SMCWinDivert64.sys"));
        assert!(!staging.exists());
    }

    #[test]
    fn signed_windivert_driver_alias_must_match_in_zip_and_staging() {
        let temp = TestDirectory::new("driver-alias");
        let mut entries = valid_entries();
        let alias = entries
            .iter_mut()
            .find(|(name, _)| name.ends_with("/WinDivert64.sys"))
            .expect("WinDivert alias fixture");
        alias.1 = b"different-driver";
        let destination = temp.0.join("mismatch");
        let error = extract_windows_bundle(&bundle(&entries), &destination)
            .expect_err("mismatched signed driver aliases must fail closed");
        assert!(error.to_string().contains("byte-identical"));
        assert!(!destination.exists());

        let valid = temp.0.join("valid");
        let extracted = extract_windows_bundle(&bundle(&valid_entries()), &valid)
            .expect("matching signed driver aliases should extract");
        fs::write(extracted.join("WinDivert64.sys"), b"changed-after-extract")
            .expect("modify staging alias");
        let error = validate_windows_bundle_tree(&extracted)
            .expect_err("staging must reject modified driver alias");
        assert!(error.to_string().contains("byte-identical"));
    }

    #[test]
    fn traversal_backslash_drive_and_outside_root_entries_are_rejected() {
        for unsafe_name in [
            "Spencer-Macro-Utilities/../escape.exe",
            "Spencer-Macro-Utilities\\escape.exe",
            "C:/Spencer-Macro-Utilities/escape.exe",
            "other-root/escape.exe",
        ] {
            let temporary = TestDirectory::new("unsafe");
            let mut entries = valid_entries();
            entries.push((unsafe_name, b"unsafe"));
            let error = extract_windows_bundle(&bundle(&entries), &temporary.0.join("stage"))
                .expect_err("unsafe entry must be rejected");
            assert!(
                error.to_string().contains("unsafe path")
                    || error.to_string().contains("expected package root")
            );
        }
    }

    #[test]
    fn duplicate_normalized_file_path_is_rejected() {
        let temporary = TestDirectory::new("duplicate");
        let mut entries = valid_entries();
        entries.push(("Spencer-Macro-Utilities//SDL3.dll", b"duplicate"));
        let error = extract_windows_bundle(&bundle(&entries), &temporary.0.join("stage"))
            .expect_err("duplicate entry must be rejected");
        assert!(error.to_string().contains("duplicate file path"));
    }

    #[test]
    fn unexpected_code_and_unknown_top_level_files_are_rejected() {
        for unexpected in [
            "Spencer-Macro-Utilities/version.dll",
            "Spencer-Macro-Utilities/assets/helper.exe",
            "Spencer-Macro-Utilities/update.cmd",
            "Spencer-Macro-Utilities/unknown.dat",
        ] {
            let temporary = TestDirectory::new("unexpected");
            let mut entries = valid_entries();
            entries.push((unexpected, b"unexpected"));
            let error = extract_windows_bundle(&bundle(&entries), &temporary.0.join("stage"))
                .expect_err("unexpected file must be rejected");
            assert!(error.to_string().contains("unexpected file"));
        }
    }

    #[test]
    fn safe_asset_and_license_extensions_remain_extensible() {
        let temporary = TestDirectory::new("extensible");
        let mut entries = valid_entries();
        entries.push(("Spencer-Macro-Utilities/assets/new-tutorial.jpeg", b"image"));
        entries.push(("Spencer-Macro-Utilities/licenses/Future-Dependency.txt", b"license"));
        let root = extract_windows_bundle(&bundle(&entries), &temporary.0.join("stage"))
            .expect("safe data extensions should remain allowed");
        assert!(root.join("assets/new-tutorial.jpeg").is_file());
        assert!(root.join("licenses/Future-Dependency.txt").is_file());
    }

    #[test]
    fn empty_required_file_is_rejected_and_partial_staging_is_removed() {
        let temporary = TestDirectory::new("empty");
        let mut entries = valid_entries();
        let entry = entries
            .iter_mut()
            .find(|(name, _)| name.ends_with("SDL3.dll"))
            .expect("SDL fixture exists");
        entry.1 = b"";
        let staging = temporary.0.join("stage");
        let error = extract_windows_bundle(&bundle(&entries), &staging)
            .expect_err("empty runtime file must be rejected");
        assert!(error.to_string().contains("empty required runtime file"));
        assert!(!staging.exists());
    }
}
