use crate::windows_bundle::{destination_path, validate_windows_bundle_tree};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(target_os = "windows")]
use std::os::windows::{
    ffi::OsStrExt as _,
    fs::{MetadataExt as _, OpenOptionsExt as _},
    io::AsRawHandle as _,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(target_os = "windows")]
use windows_sys::Win32::{
    Foundation::GENERIC_READ,
    Storage::FileSystem::{
        DELETE, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_RENAME_INFO, FILE_SHARE_READ, FileRenameInfo,
        SetFileInformationByHandle,
    },
};

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsReplacementError(String);

impl WindowsReplacementError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for WindowsReplacementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for WindowsReplacementError {}

#[derive(Debug)]
struct ReplacementRecord {
    relative: String,
    backup_source: Option<File>,
    backup_sha256: Option<[u8; 32]>,
}

#[derive(Debug)]
struct PreparedWindowsFile {
    relative: String,
    source: File,
    expected_sha256: [u8; 32],
}

#[derive(Debug)]
pub struct PreparedWindowsBundle {
    staged_root: PathBuf,
    files: Vec<PreparedWindowsFile>,
}

impl PreparedWindowsBundle {
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.staged_root
    }
}

#[derive(Debug)]
struct TargetDirectoryGuards {
    #[cfg(target_os = "windows")]
    handles: Vec<File>,
}

#[derive(Debug)]
pub struct WindowsReplacementTransaction {
    install_root: PathBuf,
    backup_root: PathBuf,
    records: Vec<ReplacementRecord>,
    directory_guards: Option<TargetDirectoryGuards>,
    active: bool,
}

impl WindowsReplacementTransaction {
    /// Commit a successful replacement by discarding rollback backups.
    ///
    /// # Errors
    ///
    /// Returns an error when the backup tree cannot be removed. The new installation remains active.
    pub fn commit(mut self) -> Result<(), WindowsReplacementError> {
        self.active = false;
        for record in &mut self.records {
            record.backup_source = None;
        }
        self.directory_guards = None;
        if self.backup_root.exists() {
            fs::remove_dir_all(&self.backup_root).map_err(|error| {
                WindowsReplacementError::new(format!(
                    "Windows update installed, but its rollback backup could not be removed: {error}"
                ))
            })?;
        }
        Ok(())
    }

    /// Restore every file changed by this transaction.
    ///
    /// # Errors
    ///
    /// Returns an error when one or more files cannot be restored.
    pub fn rollback(mut self) -> Result<(), WindowsReplacementError> {
        self.active = false;
        rollback_with_guards(
            &self.install_root,
            &self.backup_root,
            &mut self.records,
            &mut self.directory_guards,
        )
    }
}

impl Drop for WindowsReplacementTransaction {
    fn drop(&mut self) {
        if self.active {
            let _ = rollback_with_guards(
                &self.install_root,
                &self.backup_root,
                &mut self.records,
                &mut self.directory_guards,
            );
        }
    }
}

fn create_temporary_target(target: &Path) -> Result<(PathBuf, File), WindowsReplacementError> {
    let parent = target.parent().ok_or_else(|| {
        WindowsReplacementError::new("Windows update target file has no parent directory.")
    })?;
    inspect_safe_directory(parent)?;
    let stem = target.file_name().and_then(|value| value.to_str()).unwrap_or("file");
    for _ in 0..128 {
        let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".{stem}.smu-new-{}-{counter}", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(WindowsReplacementError::new(format!(
                    "Could not create a temporary Windows update target: {error}"
                )));
            }
        }
    }
    Err(WindowsReplacementError::new("Could not create a unique temporary Windows update target."))
}

fn open_staged_source(source: &Path) -> Result<File, WindowsReplacementError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "windows")]
    options.share_mode(FILE_SHARE_READ);
    options.open(source).map_err(|error| {
        WindowsReplacementError::new(format!(
            "Could not open and lock a staged Windows update file: {error}"
        ))
    })
}

fn open_original_for_backup(source: &Path) -> Result<Option<File>, WindowsReplacementError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "windows")]
    options
        .access_mode(GENERIC_READ | DELETE)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    let file = match options.open(source) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(WindowsReplacementError::new(format!(
                "Could not open and pin an installed Windows runtime file: {error}"
            )));
        }
    };
    let metadata = file.metadata().map_err(|error| {
        WindowsReplacementError::new(format!(
            "Could not inspect a pinned installed Windows runtime file: {error}"
        ))
    })?;
    if existing_target_is_unsafe(&metadata) {
        return Err(WindowsReplacementError::new(format!(
            "Windows updater refused a non-regular or reparse-point installed file: {}",
            source.display()
        )));
    }
    Ok(Some(file))
}

fn hash_open_file(source: &mut File) -> Result<[u8; 32], WindowsReplacementError> {
    source.seek(SeekFrom::Start(0)).map_err(|error| {
        WindowsReplacementError::new(format!(
            "Could not rewind a staged Windows update file: {error}"
        ))
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let count = source.read(&mut buffer).map_err(|error| {
            WindowsReplacementError::new(format!(
                "Could not read a staged Windows update file: {error}"
            ))
        })?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    source.seek(SeekFrom::Start(0)).map_err(|error| {
        WindowsReplacementError::new(format!(
            "Could not rewind a staged Windows update file: {error}"
        ))
    })?;
    Ok(hasher.finalize().into())
}

fn copy_prepared_to_temporary_target(
    source: &mut File,
    target: &Path,
    expected_sha256: &[u8; 32],
) -> Result<PathBuf, WindowsReplacementError> {
    source.seek(SeekFrom::Start(0)).map_err(|error| {
        WindowsReplacementError::new(format!(
            "Could not rewind a staged Windows update file: {error}"
        ))
    })?;
    let (temporary, mut output) = create_temporary_target(target)?;
    let result = (|| {
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
        loop {
            let count = source.read(&mut buffer).map_err(|error| {
                WindowsReplacementError::new(format!(
                    "Could not read a staged Windows update file: {error}"
                ))
            })?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
            output.write_all(&buffer[..count]).map_err(|error| {
                WindowsReplacementError::new(format!(
                    "Could not write a Windows update target file: {error}"
                ))
            })?;
        }
        let actual: [u8; 32] = hasher.finalize().into();
        if &actual != expected_sha256 {
            return Err(WindowsReplacementError::new(
                "Staged Windows update file changed after authenticated preparation.",
            ));
        }
        output.sync_all().map_err(|error| {
            WindowsReplacementError::new(format!(
                "Could not flush a Windows update target file: {error}"
            ))
        })?;
        Ok(temporary.clone())
    })();
    if result.is_err() {
        drop(output);
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn inspect_safe_directory(path: &Path) -> Result<(), WindowsReplacementError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        WindowsReplacementError::new(format!(
            "Could not inspect a Windows update target directory: {error}"
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(WindowsReplacementError::new(format!(
            "Windows updater refused a non-directory or symlink target ancestor: {}",
            path.display()
        )));
    }
    #[cfg(target_os = "windows")]
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(WindowsReplacementError::new(format!(
            "Windows updater refused a reparse-point target ancestor: {}",
            path.display()
        )));
    }
    Ok(())
}

fn ensure_safe_directory(path: &Path) -> Result<bool, WindowsReplacementError> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            inspect_safe_directory(path)?;
            Ok(false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|error| {
                WindowsReplacementError::new(format!(
                    "Could not create a Windows update target directory: {error}"
                ))
            })?;
            inspect_safe_directory(path)?;
            Ok(true)
        }
        Err(error) => Err(WindowsReplacementError::new(format!(
            "Could not inspect a Windows update target directory: {error}"
        ))),
    }
}

#[cfg_attr(not(target_os = "windows"), allow(clippy::needless_pass_by_ref_mut))]
fn pin_safe_directory(
    path: &Path,
    guards: &mut TargetDirectoryGuards,
) -> Result<(), WindowsReplacementError> {
    inspect_safe_directory(path)?;
    #[cfg(target_os = "windows")]
    {
        let mut options = OpenOptions::new();
        options
            .access_mode(0)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
        let handle = options.open(path).map_err(|error| {
            WindowsReplacementError::new(format!(
                "Could not pin a Windows update target directory: {error}"
            ))
        })?;
        let metadata = handle.metadata().map_err(|error| {
            WindowsReplacementError::new(format!(
                "Could not validate a pinned Windows update target directory: {error}"
            ))
        })?;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(WindowsReplacementError::new(format!(
                "Windows updater refused a reparse-point target ancestor: {}",
                path.display()
            )));
        }
        guards.handles.push(handle);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = guards;
    Ok(())
}

fn pin_relative_parent_chain(
    root: &Path,
    relative: &str,
    seen: &mut BTreeSet<PathBuf>,
    created: &mut Vec<PathBuf>,
    guards: &mut TargetDirectoryGuards,
) -> Result<(), WindowsReplacementError> {
    let Some(parent) = Path::new(relative).parent() else {
        return Ok(());
    };
    let mut current = root.to_path_buf();
    for component in parent.components() {
        current.push(component.as_os_str());
        if seen.insert(current.clone()) {
            if ensure_safe_directory(&current)? {
                created.push(current.clone());
            }
            pin_safe_directory(&current, guards)?;
        }
    }
    Ok(())
}

fn prepare_target_directory_guards(
    install_root: &Path,
    backup_root: &Path,
    files: &[PreparedWindowsFile],
) -> Result<TargetDirectoryGuards, WindowsReplacementError> {
    let mut guards = TargetDirectoryGuards {
        #[cfg(target_os = "windows")]
        handles: Vec::new(),
    };
    pin_safe_directory(install_root, &mut guards)?;
    let mut created = Vec::new();
    let result = (|| {
        fs::create_dir(backup_root).map_err(|error| {
            WindowsReplacementError::new(format!(
                "Could not create the Windows update rollback directory: {error}"
            ))
        })?;
        created.push(backup_root.to_path_buf());
        pin_safe_directory(backup_root, &mut guards)?;

        let mut seen = BTreeSet::from([install_root.to_path_buf(), backup_root.to_path_buf()]);
        for file in files {
            pin_relative_parent_chain(
                install_root,
                &file.relative,
                &mut seen,
                &mut created,
                &mut guards,
            )?;
            pin_relative_parent_chain(
                backup_root,
                &file.relative,
                &mut seen,
                &mut created,
                &mut guards,
            )?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        #[cfg(target_os = "windows")]
        drop(guards);
        for path in created.iter().rev() {
            let _ = fs::remove_dir(path);
        }
        return Err(error);
    }
    Ok(guards)
}

#[cfg(target_os = "windows")]
fn rename_open_file_to_backup(source: &File, backup: &Path) -> Result<(), WindowsReplacementError> {
    let file_name: Vec<u16> = backup.as_os_str().encode_wide().collect();
    let file_name_bytes = file_name
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .ok_or_else(|| WindowsReplacementError::new("Windows rollback path was too long."))?;
    let trailing_name_bytes = file_name_bytes.saturating_sub(std::mem::size_of::<u16>());
    let buffer_bytes =
        std::mem::size_of::<FILE_RENAME_INFO>().checked_add(trailing_name_bytes).ok_or_else(
            || WindowsReplacementError::new("Windows rollback rename buffer was too large."),
        )?;
    let word_bytes = std::mem::size_of::<usize>();
    let words = buffer_bytes.div_ceil(word_bytes);
    let mut storage = vec![0_usize; words];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    let file_name_length = u32::try_from(file_name_bytes)
        .map_err(|_| WindowsReplacementError::new("Windows rollback path was too long."))?;
    let buffer_size = u32::try_from(buffer_bytes).map_err(|_| {
        WindowsReplacementError::new("Windows rollback rename buffer was too large.")
    })?;
    // SAFETY: storage is usize-aligned and large enough for the variable-length FILE_RENAME_INFO plus
    // the UTF-16 filename. The source handle was opened with DELETE access and remains live for the call.
    let renamed = unsafe {
        (*info).Anonymous.ReplaceIfExists = false;
        (*info).RootDirectory = std::ptr::null_mut();
        (*info).FileNameLength = file_name_length;
        std::ptr::copy_nonoverlapping(
            file_name.as_ptr(),
            std::ptr::addr_of_mut!((*info).FileName).cast::<u16>(),
            file_name.len(),
        );
        SetFileInformationByHandle(
            source.as_raw_handle().cast(),
            FileRenameInfo,
            info.cast(),
            buffer_size,
        )
    };
    if renamed == 0 {
        return Err(WindowsReplacementError::new(format!(
            "Could not move a pinned Windows runtime file into rollback storage: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn rename_open_file_to_backup(
    _source: &File,
    backup: &Path,
    target: &Path,
) -> Result<(), WindowsReplacementError> {
    fs::rename(target, backup).map_err(|error| {
        WindowsReplacementError::new(format!(
            "Could not move an existing Windows runtime file into rollback storage: {error}"
        ))
    })
}

fn move_original_to_backup(
    target: &Path,
    backup: &Path,
) -> Result<Option<(File, [u8; 32])>, WindowsReplacementError> {
    let Some(mut source) = open_original_for_backup(target)? else {
        return Ok(None);
    };
    let digest = hash_open_file(&mut source)?;
    #[cfg(target_os = "windows")]
    rename_open_file_to_backup(&source, backup)?;
    #[cfg(not(target_os = "windows"))]
    rename_open_file_to_backup(&source, backup, target)?;
    Ok(Some((source, digest)))
}

fn existing_target_is_unsafe(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return true;
    }
    #[cfg(target_os = "windows")]
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return true;
    }
    false
}

fn restore_records(
    install_root: &Path,
    records: &mut [ReplacementRecord],
) -> Result<(), WindowsReplacementError> {
    let mut first_error = None;
    for record in records.iter_mut().rev() {
        let target = destination_path(install_root, &record.relative);
        match fs::symlink_metadata(&target) {
            Ok(metadata) if !existing_target_is_unsafe(&metadata) => {
                if let Err(error) = fs::remove_file(&target)
                    && first_error.is_none()
                {
                    first_error = Some(format!(
                        "Could not remove a partially installed update file: {error}"
                    ));
                }
            }
            Ok(_) => {
                if first_error.is_none() {
                    first_error = Some(format!(
                        "Windows update rollback refused a non-regular or reparse-point target: {}",
                        target.display()
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(format!(
                        "Could not inspect a partially installed update file: {error}"
                    ));
                }
            }
        }

        let Some(backup_source) = record.backup_source.as_mut() else {
            continue;
        };
        let Some(expected_sha256) = record.backup_sha256 else {
            if first_error.is_none() {
                first_error =
                    Some(format!("Windows update rollback digest is missing: {}", record.relative));
            }
            continue;
        };
        let temporary =
            match copy_prepared_to_temporary_target(backup_source, &target, &expected_sha256) {
                Ok(path) => path,
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(format!(
                            "Could not reconstruct a Windows update rollback file: {error}"
                        ));
                    }
                    continue;
                }
            };
        if let Err(error) = fs::rename(&temporary, &target) {
            let _ = fs::remove_file(&temporary);
            if first_error.is_none() {
                first_error =
                    Some(format!("Could not restore a Windows update rollback file: {error}"));
            }
        }
    }
    first_error.map_or(Ok(()), |error| Err(WindowsReplacementError::new(error)))
}

fn cleanup_backup_tree(backup_root: &Path) -> Result<(), WindowsReplacementError> {
    if backup_root.exists() {
        fs::remove_dir_all(backup_root).map_err(|error| {
            WindowsReplacementError::new(format!(
                "Could not remove the Windows update rollback directory: {error}"
            ))
        })?;
    }
    Ok(())
}

fn rollback_with_guards(
    install_root: &Path,
    backup_root: &Path,
    records: &mut [ReplacementRecord],
    directory_guards: &mut Option<TargetDirectoryGuards>,
) -> Result<(), WindowsReplacementError> {
    let restore = restore_records(install_root, records);
    for record in records {
        record.backup_source = None;
    }
    *directory_guards = None;
    restore?;
    cleanup_backup_tree(backup_root)
}

/// Open every validated staged file once and bind it to SHA-256 values derived from the authenticated ZIP.
///
/// On Windows, the returned file handles deny write/delete sharing for the lifetime of the prepared bundle.
///
/// # Errors
///
/// Returns an error when the staging tree differs from the authenticated file set, a staged file cannot
/// be opened, or any staged file digest differs from the authenticated ZIP digest.
pub fn prepare_windows_bundle_tree(
    staged_root: &Path,
    expected_sha256: &BTreeMap<String, [u8; 32]>,
) -> Result<PreparedWindowsBundle, WindowsReplacementError> {
    let files = validate_windows_bundle_tree(staged_root)
        .map_err(|error| WindowsReplacementError::new(error.to_string()))?;
    if files.len() != expected_sha256.len()
        || !files.iter().all(|relative| expected_sha256.contains_key(relative))
    {
        return Err(WindowsReplacementError::new(
            "Staged Windows update file set did not match the authenticated bundle.",
        ));
    }
    let staged_root = fs::canonicalize(staged_root).map_err(|error| {
        WindowsReplacementError::new(format!(
            "Could not resolve the staged Windows update root: {error}"
        ))
    })?;
    let mut prepared_files = Vec::with_capacity(files.len());
    for relative in files {
        let expected = *expected_sha256.get(&relative).ok_or_else(|| {
            WindowsReplacementError::new(
                "Staged Windows update file was missing an authenticated digest.",
            )
        })?;
        let source_path = destination_path(&staged_root, &relative);
        let mut source = open_staged_source(&source_path)?;
        let actual = hash_open_file(&mut source)?;
        if actual != expected {
            return Err(WindowsReplacementError::new(format!(
                "Staged Windows update file did not match the authenticated bundle: {relative}"
            )));
        }
        prepared_files.push(PreparedWindowsFile { relative, source, expected_sha256: expected });
    }
    Ok(PreparedWindowsBundle { staged_root, files: prepared_files })
}

fn validate_replacement_roots(
    staged_root: &Path,
    install_root: &Path,
    backup_root: &Path,
) -> Result<(PathBuf, PathBuf), WindowsReplacementError> {
    let staged = fs::canonicalize(staged_root).map_err(|error| {
        WindowsReplacementError::new(format!(
            "Could not resolve the staged Windows update root: {error}"
        ))
    })?;
    let install = fs::canonicalize(install_root).map_err(|error| {
        WindowsReplacementError::new(format!(
            "Could not resolve the Windows installation directory: {error}"
        ))
    })?;
    if staged == install || staged.starts_with(&install) || install.starts_with(&staged) {
        return Err(WindowsReplacementError::new(
            "Windows updater refused overlapping staging and installation directories.",
        ));
    }
    let backup_parent = backup_root.parent().ok_or_else(|| {
        WindowsReplacementError::new("Windows updater rollback path has no parent directory.")
    })?;
    let backup_parent = fs::canonicalize(backup_parent).map_err(|error| {
        WindowsReplacementError::new(format!(
            "Could not resolve the Windows updater rollback parent: {error}"
        ))
    })?;
    if backup_parent != install || backup_root.exists() {
        return Err(WindowsReplacementError::new(
            "Windows updater rollback directory must be a new direct child of the installation directory.",
        ));
    }
    Ok((staged, install))
}

/// Replace the validated Rust Windows bundle files while preserving rollback copies of every overwritten
/// file. User/profile files outside the frozen bundle layout are never touched.
///
/// # Errors
///
/// Returns an error before or during replacement. Any files already changed by this call are rolled back
/// best-effort before the error is returned.
pub fn replace_windows_bundle_tree(
    prepared: &mut PreparedWindowsBundle,
    install_root: &Path,
    backup_root: &Path,
) -> Result<WindowsReplacementTransaction, WindowsReplacementError> {
    let (_, install_root) =
        validate_replacement_roots(&prepared.staged_root, install_root, backup_root)?;
    let mut directory_guards =
        Some(prepare_target_directory_guards(&install_root, backup_root, &prepared.files)?);

    let mut records = Vec::with_capacity(prepared.files.len());
    for file in &mut prepared.files {
        let relative = file.relative.clone();
        let target = destination_path(&install_root, &relative);
        let backup = destination_path(backup_root, &relative);
        let (backup_source, backup_sha256) = match move_original_to_backup(&target, &backup) {
            Ok(Some((source, digest))) => (Some(source), Some(digest)),
            Ok(None) => (None, None),
            Err(error) => {
                let _ = rollback_with_guards(
                    &install_root,
                    backup_root,
                    &mut records,
                    &mut directory_guards,
                );
                return Err(error);
            }
        };
        records.push(ReplacementRecord {
            relative: relative.clone(),
            backup_source,
            backup_sha256,
        });

        let temporary = match copy_prepared_to_temporary_target(
            &mut file.source,
            &target,
            &file.expected_sha256,
        ) {
            Ok(path) => path,
            Err(error) => {
                let _ = rollback_with_guards(
                    &install_root,
                    backup_root,
                    &mut records,
                    &mut directory_guards,
                );
                return Err(error);
            }
        };
        if let Err(error) = fs::rename(&temporary, &target) {
            let _ = fs::remove_file(&temporary);
            let _ = rollback_with_guards(
                &install_root,
                backup_root,
                &mut records,
                &mut directory_guards,
            );
            return Err(WindowsReplacementError::new(format!(
                "Could not activate a staged Windows update file: {error}"
            )));
        }
    }

    Ok(WindowsReplacementTransaction {
        install_root,
        backup_root: backup_root.to_path_buf(),
        records,
        directory_guards,
        active: true,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::WINDOWS_BUNDLE_ROOT;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let counter = TEMP_COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("smu-windows-replace-{label}-{}-{counter}", std::process::id()));
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

    fn write_file(root: &Path, relative: &str, contents: &[u8]) {
        let path = destination_path(root, relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent should be created");
        }
        fs::write(path, contents).expect("fixture should be written");
    }

    fn staged_tree(base: &Path) -> PathBuf {
        let root = base.join("staging").join(WINDOWS_BUNDLE_ROOT);
        write_file(&root, "Spencer-Macro-Utilities.exe", b"new-exe");
        write_file(&root, "SMU-Update-Helper.exe", b"new-helper");
        write_file(&root, "SDL3.dll", b"new-sdl");
        write_file(&root, "SMCWinDivert.dll", b"new-windivert");
        write_file(&root, "SMCWinDivert64.sys", b"new-driver");
        write_file(&root, "assets/LSANS.TTF", b"new-font");
        write_file(&root, "assets/macro_tutorials/wallhop.jpg", b"new-image");
        write_file(&root, "LICENSE", b"new-license");
        root
    }

    fn hashes_for_tree(root: &Path) -> BTreeMap<String, [u8; 32]> {
        validate_windows_bundle_tree(root)
            .expect("staged tree should validate")
            .into_iter()
            .map(|relative| {
                let bytes = fs::read(destination_path(root, &relative))
                    .expect("staged fixture should be readable");
                (relative, Sha256::digest(bytes).into())
            })
            .collect()
    }

    fn prepared_tree(root: &Path) -> PreparedWindowsBundle {
        prepare_windows_bundle_tree(root, &hashes_for_tree(root))
            .expect("staged tree should prepare")
    }

    #[test]
    fn replacement_commit_updates_managed_files_and_preserves_user_files() {
        let temp = TestDirectory::new("commit");
        let staged = staged_tree(&temp.0);
        let install = temp.0.join("install");
        fs::create_dir_all(&install).expect("install root should exist");
        write_file(&install, "Spencer-Macro-Utilities.exe", b"old-exe");
        write_file(&install, "SDL3.dll", b"old-sdl");
        write_file(&install, "profiles.json", b"user-data");
        let backup = install.join(".smu-update-backup-test");
        let mut prepared = prepared_tree(&staged);

        let transaction = replace_windows_bundle_tree(&mut prepared, &install, &backup)
            .expect("replacement should succeed");
        assert_eq!(
            fs::read(install.join("Spencer-Macro-Utilities.exe"))
                .expect("fixture file should read"),
            b"new-exe"
        );
        assert_eq!(
            fs::read(install.join("SDL3.dll")).expect("fixture file should read"),
            b"new-sdl"
        );
        assert_eq!(
            fs::read(install.join("profiles.json")).expect("fixture file should read"),
            b"user-data"
        );
        assert_eq!(
            fs::read(backup.join("Spencer-Macro-Utilities.exe")).expect("fixture file should read"),
            b"old-exe"
        );

        transaction.commit().expect("commit should remove rollback files");
        assert!(!backup.exists());
        assert_eq!(
            fs::read(install.join("profiles.json")).expect("fixture file should read"),
            b"user-data"
        );
    }

    #[test]
    fn explicit_rollback_restores_old_files_and_removes_new_files() {
        let temp = TestDirectory::new("rollback");
        let staged = staged_tree(&temp.0);
        let install = temp.0.join("install");
        fs::create_dir_all(&install).expect("install root should exist");
        write_file(&install, "Spencer-Macro-Utilities.exe", b"old-exe");
        let backup = install.join(".smu-update-backup-test");
        let mut prepared = prepared_tree(&staged);

        let transaction = replace_windows_bundle_tree(&mut prepared, &install, &backup)
            .expect("replacement should succeed");
        transaction.rollback().expect("rollback should succeed");

        assert_eq!(
            fs::read(install.join("Spencer-Macro-Utilities.exe"))
                .expect("fixture file should read"),
            b"old-exe"
        );
        assert!(!install.join("SDL3.dll").exists());
        assert!(!backup.exists());
    }

    // This fixture relies on Unix allowing mutation/unlink of open file handles.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn rollback_uses_pinned_original_even_if_backup_path_is_removed() {
        let temp = TestDirectory::new("unlinked-backup");
        let staged = staged_tree(&temp.0);
        let install = temp.0.join("install");
        fs::create_dir_all(&install).expect("install root should exist");
        write_file(&install, "Spencer-Macro-Utilities.exe", b"old-exe");
        let backup = install.join(".smu-update-backup-test");
        let mut prepared = prepared_tree(&staged);

        let transaction = replace_windows_bundle_tree(&mut prepared, &install, &backup)
            .expect("replacement should succeed");
        fs::remove_file(backup.join("Spencer-Macro-Utilities.exe"))
            .expect("rollback fixture should unlink the backup pathname");
        transaction.rollback().expect("rollback should restore from the pinned original handle");

        assert_eq!(
            fs::read(install.join("Spencer-Macro-Utilities.exe"))
                .expect("restored fixture should read"),
            b"old-exe"
        );
        assert!(!backup.exists());
    }

    // This fixture relies on Unix allowing mutation/unlink of open file handles.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn rollback_refuses_modified_backup_bytes() {
        let temp = TestDirectory::new("modified-backup");
        let staged = staged_tree(&temp.0);
        let install = temp.0.join("install");
        fs::create_dir_all(&install).expect("install root should exist");
        write_file(&install, "Spencer-Macro-Utilities.exe", b"old-exe");
        let backup = install.join(".smu-update-backup-test");
        let mut prepared = prepared_tree(&staged);

        let transaction = replace_windows_bundle_tree(&mut prepared, &install, &backup)
            .expect("replacement should succeed");
        fs::write(backup.join("Spencer-Macro-Utilities.exe"), b"tampered")
            .expect("non-Windows fixture can mutate the backup inode");
        let error = transaction.rollback().expect_err("modified rollback bytes must fail closed");

        assert!(error.to_string().contains("authenticated preparation"));
        assert!(!install.join("Spencer-Macro-Utilities.exe").exists());
    }

    #[test]
    fn mid_replacement_failure_rolls_back_files_already_changed() {
        let temp = TestDirectory::new("failure");
        let staged = staged_tree(&temp.0);
        let install = temp.0.join("install");
        fs::create_dir_all(&install).expect("install root should exist");
        write_file(&install, "SDL3.dll", b"old-sdl");
        write_file(&install, "SMCWinDivert.dll", b"old-windivert");
        write_file(&install, "Spencer-Macro-Utilities.exe", b"old-exe");
        fs::write(install.join("assets"), b"parent-is-a-file")
            .expect("collision fixture should be written");
        let backup = install.join(".smu-update-backup-test");
        let mut prepared = prepared_tree(&staged);

        let error = replace_windows_bundle_tree(&mut prepared, &install, &backup)
            .expect_err("asset parent collision should fail replacement");
        assert!(error.to_string().contains("target ancestor"));
        assert_eq!(
            fs::read(install.join("SDL3.dll")).expect("fixture file should read"),
            b"old-sdl"
        );
        assert_eq!(
            fs::read(install.join("SMCWinDivert.dll")).expect("fixture file should read"),
            b"old-windivert"
        );
        assert_eq!(
            fs::read(install.join("Spencer-Macro-Utilities.exe"))
                .expect("fixture file should read"),
            b"old-exe"
        );
        assert!(!backup.exists());
    }

    #[test]
    fn authenticated_digest_mismatch_is_rejected_during_preparation() {
        let temp = TestDirectory::new("prepare-hash-mismatch");
        let staged = staged_tree(&temp.0);
        let mut hashes = hashes_for_tree(&staged);
        hashes.insert("SDL3.dll".to_owned(), [0_u8; 32]);

        let error = prepare_windows_bundle_tree(&staged, &hashes)
            .expect_err("digest mismatch must reject staged tree");
        assert!(error.to_string().contains("did not match the authenticated bundle"));
    }

    // This fixture relies on Unix allowing mutation/unlink of open file handles.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn staged_mutation_after_preparation_is_detected_before_activation() {
        let temp = TestDirectory::new("prepared-mutation");
        let staged = staged_tree(&temp.0);
        let install = temp.0.join("install");
        fs::create_dir_all(&install).expect("install root should exist");
        write_file(&install, "Spencer-Macro-Utilities.exe", b"old-exe");
        write_file(&install, "SDL3.dll", b"old-sdl");
        let backup = install.join(".smu-update-backup-test");
        let mut prepared = prepared_tree(&staged);

        write_file(&staged, "SDL3.dll", b"tampered-after-prepare");
        let error = replace_windows_bundle_tree(&mut prepared, &install, &backup)
            .expect_err("post-preparation mutation must fail replacement");
        assert!(error.to_string().contains("changed after authenticated preparation"));
        assert_eq!(
            fs::read(install.join("Spencer-Macro-Utilities.exe"))
                .expect("fixture file should read"),
            b"old-exe"
        );
        assert_eq!(
            fs::read(install.join("SDL3.dll")).expect("fixture file should read"),
            b"old-sdl"
        );
        assert!(!backup.exists());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn prepared_sources_and_backup_are_pinned_against_windows_file_mutation() {
        let temp = TestDirectory::new("windows-sharing");
        let staged = staged_tree(&temp.0);
        let install = temp.0.join("install");
        fs::create_dir_all(&install).expect("install root should exist");
        write_file(&install, "Spencer-Macro-Utilities.exe", b"old-exe");
        let backup = install.join(".smu-update-backup-test");
        let mut prepared = prepared_tree(&staged);

        // On Windows, the authenticated source is pinned with FILE_SHARE_READ.
        // Attempts to modify or delete it must fail with sharing violations.
        let mutation = fs::write(staged.join("SDL3.dll"), b"tampered")
            .expect_err("a pinned source must not be writable");
        assert_eq!(mutation.raw_os_error(), Some(32)); // ERROR_SHARING_VIOLATION

        let transaction = replace_windows_bundle_tree(&mut prepared, &install, &backup)
            .expect("replacement should succeed");
        let backup_path = backup.join("Spencer-Macro-Utilities.exe");
        let mutation = fs::write(&backup_path, b"tampered")
            .expect_err("pinned rollback backup must not be writable");
        assert_eq!(mutation.raw_os_error(), Some(32));
        let deletion =
            fs::remove_file(&backup_path).expect_err("pinned rollback backup must not be unlinked");
        assert_eq!(deletion.raw_os_error(), Some(32));
        transaction.rollback().expect("untampered rollback should succeed");
        assert_eq!(
            fs::read(install.join("Spencer-Macro-Utilities.exe"))
                .expect("restored original should be readable"),
            b"old-exe"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_target_ancestor_is_rejected_before_replacement() {
        use std::os::unix::fs::symlink;

        let temp = TestDirectory::new("target-symlink");
        let staged = staged_tree(&temp.0);
        let install = temp.0.join("install");
        let outside = temp.0.join("outside");
        fs::create_dir_all(&install).expect("install root should exist");
        fs::create_dir_all(&outside).expect("outside root should exist");
        write_file(&outside, "sentinel.txt", b"outside");
        symlink(&outside, install.join("assets")).expect("ancestor symlink should be created");
        let backup = install.join(".smu-update-backup-test");
        let mut prepared = prepared_tree(&staged);

        let error = replace_windows_bundle_tree(&mut prepared, &install, &backup)
            .expect_err("symlinked target ancestor must be rejected");
        assert!(error.to_string().contains("target ancestor"));
        assert_eq!(
            fs::read(outside.join("sentinel.txt")).expect("outside sentinel should read"),
            b"outside"
        );
        assert!(!backup.exists());
    }

    #[test]
    fn overlapping_roots_and_existing_backup_are_rejected() {
        let temp = TestDirectory::new("roots");
        let staged = staged_tree(&temp.0);
        let install = temp.0.join("install");
        fs::create_dir_all(&install).expect("install root should exist");
        let backup = install.join(".smu-update-backup-test");
        fs::create_dir_all(&backup).expect("backup collision should exist");
        let mut prepared = prepared_tree(&staged);
        assert!(replace_windows_bundle_tree(&mut prepared, &install, &backup).is_err());

        let nested_install = staged.join("nested-install");
        fs::create_dir_all(&nested_install).expect("nested install should exist");
        let nested_backup = nested_install.join(".smu-update-backup-test");
        assert!(
            replace_windows_bundle_tree(&mut prepared, &nested_install, &nested_backup).is_err()
        );
    }
}
