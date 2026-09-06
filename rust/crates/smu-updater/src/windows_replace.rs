use crate::windows_bundle::{destination_path, validate_windows_bundle_tree};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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

#[derive(Clone, Debug)]
struct ReplacementRecord {
    relative: String,
    had_original: bool,
}

#[derive(Debug)]
pub struct WindowsReplacementTransaction {
    install_root: PathBuf,
    backup_root: PathBuf,
    records: Vec<ReplacementRecord>,
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
        rollback_records(&self.install_root, &self.backup_root, &self.records)
    }
}

impl Drop for WindowsReplacementTransaction {
    fn drop(&mut self) {
        if self.active {
            let _ = rollback_records(&self.install_root, &self.backup_root, &self.records);
        }
    }
}

fn create_temporary_target(target: &Path) -> Result<(PathBuf, File), WindowsReplacementError> {
    let parent = target.parent().ok_or_else(|| {
        WindowsReplacementError::new("Windows update target file has no parent directory.")
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        WindowsReplacementError::new(format!(
            "Could not create a Windows update target directory: {error}"
        ))
    })?;
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

fn copy_to_temporary_target(
    source: &Path,
    target: &Path,
) -> Result<PathBuf, WindowsReplacementError> {
    let mut source_file = File::open(source).map_err(|error| {
        WindowsReplacementError::new(format!(
            "Could not open a staged Windows update file: {error}"
        ))
    })?;
    let (temporary, mut output) = create_temporary_target(target)?;
    let result = (|| {
        let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
        loop {
            let count = source_file.read(&mut buffer).map_err(|error| {
                WindowsReplacementError::new(format!(
                    "Could not read a staged Windows update file: {error}"
                ))
            })?;
            if count == 0 {
                break;
            }
            output.write_all(&buffer[..count]).map_err(|error| {
                WindowsReplacementError::new(format!(
                    "Could not write a Windows update target file: {error}"
                ))
            })?;
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

fn reject_unsafe_existing_target(target: &Path) -> Result<bool, WindowsReplacementError> {
    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(WindowsReplacementError::new(format!(
                "Windows updater refused to replace a non-regular target: {}",
                target.display()
            )))
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(WindowsReplacementError::new(format!(
            "Could not inspect an existing Windows update target: {error}"
        ))),
    }
}

fn rollback_records(
    install_root: &Path,
    backup_root: &Path,
    records: &[ReplacementRecord],
) -> Result<(), WindowsReplacementError> {
    let mut first_error = None;
    for record in records.iter().rev() {
        let target = destination_path(install_root, &record.relative);
        if target.exists()
            && let Err(error) = fs::remove_file(&target)
            && first_error.is_none()
        {
            first_error =
                Some(format!("Could not remove a partially installed update file: {error}"));
        }
        if record.had_original {
            let backup = destination_path(backup_root, &record.relative);
            if let Some(parent) = target.parent()
                && let Err(error) = fs::create_dir_all(parent)
                && first_error.is_none()
            {
                first_error =
                    Some(format!("Could not recreate a rollback target directory: {error}"));
            }
            if !backup.exists() {
                if first_error.is_none() {
                    first_error = Some(format!(
                        "Windows update rollback backup is missing: {}",
                        backup.display()
                    ));
                }
                continue;
            }
            if let Err(error) = fs::rename(&backup, &target)
                && first_error.is_none()
            {
                first_error =
                    Some(format!("Could not restore a Windows update rollback file: {error}"));
            }
        }
    }
    if backup_root.exists()
        && let Err(error) = fs::remove_dir_all(backup_root)
        && first_error.is_none()
    {
        first_error =
            Some(format!("Could not remove the Windows update rollback directory: {error}"));
    }
    first_error.map_or(Ok(()), |error| Err(WindowsReplacementError::new(error)))
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
    staged_root: &Path,
    install_root: &Path,
    backup_root: &Path,
) -> Result<WindowsReplacementTransaction, WindowsReplacementError> {
    let files = validate_windows_bundle_tree(staged_root)
        .map_err(|error| WindowsReplacementError::new(error.to_string()))?;
    let (staged_root, install_root) =
        validate_replacement_roots(staged_root, install_root, backup_root)?;
    fs::create_dir(backup_root).map_err(|error| {
        WindowsReplacementError::new(format!(
            "Could not create the Windows update rollback directory: {error}"
        ))
    })?;

    let mut records = Vec::with_capacity(files.len());
    for relative in files {
        let source = destination_path(&staged_root, &relative);
        let target = destination_path(&install_root, &relative);
        let backup = destination_path(backup_root, &relative);
        let had_original = match reject_unsafe_existing_target(&target) {
            Ok(value) => value,
            Err(error) => {
                let _ = rollback_records(&install_root, backup_root, &records);
                return Err(error);
            }
        };
        if had_original {
            if let Some(parent) = backup.parent()
                && let Err(error) = fs::create_dir_all(parent)
            {
                let _ = rollback_records(&install_root, backup_root, &records);
                return Err(WindowsReplacementError::new(format!(
                    "Could not create a Windows update rollback directory: {error}"
                )));
            }
            if let Err(error) = fs::rename(&target, &backup) {
                let _ = rollback_records(&install_root, backup_root, &records);
                return Err(WindowsReplacementError::new(format!(
                    "Could not move an existing Windows runtime file into rollback storage: {error}"
                )));
            }
        }
        records.push(ReplacementRecord { relative: relative.clone(), had_original });

        let temporary = match copy_to_temporary_target(&source, &target) {
            Ok(path) => path,
            Err(error) => {
                let _ = rollback_records(&install_root, backup_root, &records);
                return Err(error);
            }
        };
        if let Err(error) = fs::rename(&temporary, &target) {
            let _ = fs::remove_file(&temporary);
            let _ = rollback_records(&install_root, backup_root, &records);
            return Err(WindowsReplacementError::new(format!(
                "Could not activate a staged Windows update file: {error}"
            )));
        }
    }

    Ok(WindowsReplacementTransaction {
        install_root,
        backup_root: backup_root.to_path_buf(),
        records,
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
        write_file(&root, "SDL3.dll", b"new-sdl");
        write_file(&root, "SMCWinDivert.dll", b"new-windivert");
        write_file(&root, "SMCWinDivert64.sys", b"new-driver");
        write_file(&root, "assets/LSANS.TTF", b"new-font");
        write_file(&root, "assets/macro_tutorials/wallhop.jpg", b"new-image");
        write_file(&root, "LICENSE", b"new-license");
        root
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

        let transaction = replace_windows_bundle_tree(&staged, &install, &backup)
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

        let transaction = replace_windows_bundle_tree(&staged, &install, &backup)
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

    #[test]
    fn rollback_reports_missing_original_backup_instead_of_succeeding_silently() {
        let temp = TestDirectory::new("missing-backup");
        let staged = staged_tree(&temp.0);
        let install = temp.0.join("install");
        fs::create_dir_all(&install).expect("install root should exist");
        write_file(&install, "Spencer-Macro-Utilities.exe", b"old-exe");
        let backup = install.join(".smu-update-backup-test");

        let transaction = replace_windows_bundle_tree(&staged, &install, &backup)
            .expect("replacement should succeed");
        fs::remove_file(backup.join("Spencer-Macro-Utilities.exe"))
            .expect("rollback fixture should remove the original backup");
        let error = transaction.rollback().expect_err("missing backup must fail rollback");

        assert!(error.to_string().contains("rollback backup is missing"));
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

        let error = replace_windows_bundle_tree(&staged, &install, &backup)
            .expect_err("asset parent collision should fail replacement");
        assert!(error.to_string().contains("Windows update target"));
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
    fn overlapping_roots_and_existing_backup_are_rejected() {
        let temp = TestDirectory::new("roots");
        let staged = staged_tree(&temp.0);
        let install = temp.0.join("install");
        fs::create_dir_all(&install).expect("install root should exist");
        let backup = install.join(".smu-update-backup-test");
        fs::create_dir_all(&backup).expect("backup collision should exist");
        assert!(replace_windows_bundle_tree(&staged, &install, &backup).is_err());

        let nested_install = staged.join("nested-install");
        fs::create_dir_all(&nested_install).expect("nested install should exist");
        let nested_backup = nested_install.join(".smu-update-backup-test");
        assert!(replace_windows_bundle_tree(&staged, &nested_install, &nested_backup).is_err());
    }
}
