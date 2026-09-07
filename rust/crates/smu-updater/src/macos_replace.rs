use std::ffi::{CStr, CString, OsStr, OsString};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd as _, FromRawFd as _};
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DirectoryIdentity {
    device: u64,
    inode: u64,
}

impl DirectoryIdentity {
    fn from_file(file: &File) -> Result<Self, MacosReplaceError> {
        let metadata = file.metadata().map_err(|error| {
            MacosReplaceError::new(format!(
                "Could not inspect pinned macOS update directory: {error}"
            ))
        })?;
        if !metadata.is_dir() {
            return Err(MacosReplaceError::new(
                "Refusing a pinned macOS update path that is not a directory.",
            ));
        }
        Ok(Self { device: metadata.dev(), inode: metadata.ino() })
    }
}

#[derive(Clone, Copy, Debug)]
struct ChildMetadata {
    identity: DirectoryIdentity,
    mode: libc::mode_t,
}

impl ChildMetadata {
    const fn is_directory(self) -> bool {
        self.mode & libc::S_IFMT == libc::S_IFDIR
    }
}

#[derive(Debug)]
pub struct MacosReplaceError {
    message: String,
    recovery: Option<Box<RecoveryState>>,
}

impl MacosReplaceError {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), recovery: None }
    }

    fn with_recovery(message: impl Into<String>, recovery: RecoveryState) -> Self {
        Self { message: message.into(), recovery: Some(Box::new(recovery)) }
    }

    #[must_use]
    pub const fn recovery_available(&self) -> bool {
        self.recovery.is_some()
    }

    /// Retry safe restoration after an activation error that retained rollback ownership.
    ///
    /// # Errors
    ///
    /// Returns the original error when no recovery state exists, or a new recovery-bearing error when the
    /// descriptor-relative identity checks still prevent safe restoration.
    pub fn recover(mut self) -> Result<(), Self> {
        let Some(mut recovery) = self.recovery.take() else {
            return Err(self);
        };
        match recovery.try_recover() {
            Ok(()) => Ok(()),
            Err(error) => Err(Self { message: error.to_string(), recovery: Some(recovery) }),
        }
    }
}

impl fmt::Display for MacosReplaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for MacosReplaceError {}

impl Drop for MacosReplaceError {
    fn drop(&mut self) {
        if let Some(mut recovery) = self.recovery.take() {
            let _ = recovery.try_recover();
        }
    }
}

fn c_name(name: &OsStr) -> Result<CString, MacosReplaceError> {
    CString::new(name.as_bytes()).map_err(|_| {
        MacosReplaceError::new("Refusing a macOS update filename containing an embedded NUL byte.")
    })
}

fn open_pinned_directory(path: &Path) -> Result<File, MacosReplaceError> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| {
            MacosReplaceError::new(format!(
                "Could not pin macOS update directory {}: {error}",
                path.display()
            ))
        })
}

fn child_metadata(dir: &File, name: &OsStr) -> Result<Option<ChildMetadata>, MacosReplaceError> {
    let name = c_name(name)?;
    let mut stat = MaybeUninit::<libc::stat>::zeroed();
    // SAFETY: dir is a live directory descriptor, name is NUL-terminated, and stat points to writable storage.
    let result = unsafe {
        libc::fstatat(dir.as_raw_fd(), name.as_ptr(), stat.as_mut_ptr(), libc::AT_SYMLINK_NOFOLLOW)
    };
    if result != 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ENOENT) {
            return Ok(None);
        }
        return Err(MacosReplaceError::new(format!(
            "Could not inspect descriptor-relative macOS update entry: {error}"
        )));
    }
    // SAFETY: successful fstatat initialized the entire stat structure.
    let stat = unsafe { stat.assume_init() };
    // st_dev/st_ino widths differ across Unix targets; normalize both to the identity type used here.
    #[allow(clippy::unnecessary_cast)]
    let identity = DirectoryIdentity { device: stat.st_dev as u64, inode: stat.st_ino as u64 };
    Ok(Some(ChildMetadata { identity, mode: stat.st_mode }))
}

fn require_child_directory_identity(
    dir: &File,
    name: &OsStr,
    expected: DirectoryIdentity,
    label: &str,
) -> Result<(), MacosReplaceError> {
    let metadata = child_metadata(dir, name)?.ok_or_else(|| {
        MacosReplaceError::new(format!(
            "Refusing macOS update because the {label} bundle is missing."
        ))
    })?;
    if !metadata.is_directory() || metadata.identity != expected {
        return Err(MacosReplaceError::new(format!(
            "Refusing macOS update because the {label} bundle changed identity."
        )));
    }
    Ok(())
}

fn require_child_absent(dir: &File, name: &OsStr, label: &str) -> Result<(), MacosReplaceError> {
    if child_metadata(dir, name)?.is_some() {
        return Err(MacosReplaceError::new(format!(
            "Refusing macOS update because the {label} path already exists."
        )));
    }
    Ok(())
}

fn rename_at(
    source_dir: &File,
    source_name: &OsStr,
    target_dir: &File,
    target_name: &OsStr,
) -> Result<(), MacosReplaceError> {
    let source_name = c_name(source_name)?;
    let target_name = c_name(target_name)?;
    // SAFETY: both descriptors remain live and both names are valid NUL-terminated relative child names.
    let result = unsafe {
        libc::renameat(
            source_dir.as_raw_fd(),
            source_name.as_ptr(),
            target_dir.as_raw_fd(),
            target_name.as_ptr(),
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(MacosReplaceError::new(format!(
            "Descriptor-relative macOS app rename failed: {}",
            io::Error::last_os_error()
        )))
    }
}

fn open_child_directory(dir: &File, name: &OsStr) -> Result<File, MacosReplaceError> {
    let name = c_name(name)?;
    // SAFETY: dir remains live and name is a valid relative child name. O_NOFOLLOW rejects a symlink root.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(MacosReplaceError::new(format!(
            "Could not open descriptor-relative macOS update directory: {}",
            io::Error::last_os_error()
        )));
    }
    // SAFETY: fd is a newly-owned descriptor returned by openat on success.
    Ok(unsafe { File::from_raw_fd(fd) })
}

struct DirectoryStream(*mut libc::DIR);

impl Drop for DirectoryStream {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this guard uniquely owns the DIR* returned by fdopendir.
            unsafe { libc::closedir(self.0) };
        }
    }
}

fn directory_entries(dir: &File) -> Result<Vec<OsString>, MacosReplaceError> {
    // SAFETY: dup returns a new descriptor referring to the same pinned directory object.
    let duplicate = unsafe { libc::dup(dir.as_raw_fd()) };
    if duplicate < 0 {
        return Err(MacosReplaceError::new(format!(
            "Could not duplicate pinned macOS update directory: {}",
            io::Error::last_os_error()
        )));
    }
    // SAFETY: duplicate is a live directory descriptor and ownership transfers to fdopendir on success.
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        // SAFETY: fdopendir failed, so ownership of duplicate did not transfer.
        unsafe { libc::close(duplicate) };
        return Err(MacosReplaceError::new(format!(
            "Could not enumerate pinned macOS update directory: {}",
            io::Error::last_os_error()
        )));
    }
    let stream = DirectoryStream(stream);
    let mut entries = Vec::new();
    loop {
        // SAFETY: stream owns a valid DIR* for the duration of enumeration.
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            break;
        }
        // SAFETY: readdir returned a valid dirent whose d_name is NUL-terminated for this call.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if name == b"." || name == b".." {
            continue;
        }
        entries.push(OsString::from_vec(name.to_vec()));
    }
    Ok(entries)
}

fn unlink_at(dir: &File, name: &OsStr, flags: i32) -> Result<(), MacosReplaceError> {
    let name = c_name(name)?;
    // SAFETY: dir remains live and name is a valid descriptor-relative child name.
    let result = unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), flags) };
    if result == 0 {
        Ok(())
    } else {
        Err(MacosReplaceError::new(format!(
            "Could not remove descriptor-relative macOS update entry: {}",
            io::Error::last_os_error()
        )))
    }
}

fn open_verified_child_directory(
    parent: &File,
    name: &OsStr,
    expected: DirectoryIdentity,
    label: &str,
) -> Result<File, MacosReplaceError> {
    require_child_directory_identity(parent, name, expected, label)?;
    let directory = open_child_directory(parent, name)?;
    if DirectoryIdentity::from_file(&directory)? != expected {
        return Err(MacosReplaceError::new(format!(
            "Refusing macOS update because the {label} directory changed identity after opening."
        )));
    }
    Ok(directory)
}

fn remove_open_directory_tree_at(
    parent: &File,
    name: &OsStr,
    expected: DirectoryIdentity,
    directory: &File,
) -> Result<(), MacosReplaceError> {
    if DirectoryIdentity::from_file(directory)? != expected {
        return Err(MacosReplaceError::new(
            "Refusing macOS update cleanup because the pinned root directory changed identity.",
        ));
    }
    remove_directory_contents(directory, expected.device)?;
    // macOS has no unlink-by-directory-handle primitive. Recursive deletion is therefore performed only through
    // the verified root FD, and the now-empty root directory itself is intentionally retained. Recheck its name
    // for diagnostics, but never race a final path-based rmdir against a same-UID rename/substitution.
    require_child_directory_identity(parent, name, expected, "emptied cleanup")
}

fn remove_directory_tree_at(
    parent: &File,
    name: &OsStr,
    expected: DirectoryIdentity,
) -> Result<(), MacosReplaceError> {
    let directory = open_verified_child_directory(parent, name, expected, "cleanup")?;
    remove_open_directory_tree_at(parent, name, expected, &directory)
}

fn remove_directory_contents(dir: &File, root_device: u64) -> Result<(), MacosReplaceError> {
    for name in directory_entries(dir)? {
        let metadata = child_metadata(dir, &name)?.ok_or_else(|| {
            MacosReplaceError::new("macOS update cleanup entry disappeared during enumeration.")
        })?;
        if metadata.is_directory() {
            if metadata.identity.device != root_device {
                return Err(MacosReplaceError::new(
                    "Refusing macOS update cleanup across a mounted filesystem boundary.",
                ));
            }
            let child = open_child_directory(dir, &name)?;
            if DirectoryIdentity::from_file(&child)? != metadata.identity {
                return Err(MacosReplaceError::new(
                    "Refusing macOS update cleanup because a child directory changed identity after opening.",
                ));
            }
            remove_directory_contents(&child, root_device)?;
            unlink_at(dir, &name, libc::AT_REMOVEDIR)?;
        } else {
            unlink_at(dir, &name, 0)?;
        }
    }
    Ok(())
}

fn finalize_swap_container(
    swap: &File,
    swap_identity: DirectoryIdentity,
) -> Result<(), MacosReplaceError> {
    if DirectoryIdentity::from_file(swap)? != swap_identity {
        return Err(MacosReplaceError::new("Pinned macOS update swap directory changed identity."));
    }
    // The swap root and emptied child roots are intentionally retained. macOS provides no safe unlink-by-handle
    // primitive, so attempting a final pathname rmdir would reopen a same-UID TOCTOU after descriptor-relative
    // cleanup has already succeeded. Empty hidden directories are preferable to deleting a substituted name.
    Ok(())
}

#[derive(Debug)]
pub struct PreparedMacosReplacement {
    parent: Option<File>,
    swap: Option<File>,
    swap_identity: DirectoryIdentity,
    current_name: OsString,
    new_name: OsString,
    backup_name: OsString,
    old_identity: DirectoryIdentity,
    new_identity: DirectoryIdentity,
    armed: bool,
}

impl PreparedMacosReplacement {
    pub(crate) fn from_paths(
        swap_dir: &Path,
        current_app: &Path,
        new_app: &Path,
        backup_app: &Path,
        expected_old_identity: (u64, u64),
        expected_new_identity: (u64, u64),
    ) -> Result<Self, MacosReplaceError> {
        let parent_path = current_app.parent().ok_or_else(|| {
            MacosReplaceError::new("Could not resolve the installed macOS app parent directory.")
        })?;
        if swap_dir.parent() != Some(parent_path)
            || new_app.parent() != Some(swap_dir)
            || backup_app.parent() != Some(swap_dir)
        {
            return Err(MacosReplaceError::new(
                "macOS update paths do not share the required install/swap directory relationship.",
            ));
        }
        let current_name = current_app.file_name().ok_or_else(|| {
            MacosReplaceError::new("Installed macOS app path has no final component.")
        })?;
        let new_name = new_app.file_name().ok_or_else(|| {
            MacosReplaceError::new("Prepared macOS app path has no final component.")
        })?;
        let backup_name = backup_app.file_name().ok_or_else(|| {
            MacosReplaceError::new("Rollback macOS app path has no final component.")
        })?;
        let parent = open_pinned_directory(parent_path)?;
        let swap = open_pinned_directory(swap_dir)?;
        let swap_identity = DirectoryIdentity::from_file(&swap)?;
        let old_identity =
            DirectoryIdentity { device: expected_old_identity.0, inode: expected_old_identity.1 };
        let new_identity =
            DirectoryIdentity { device: expected_new_identity.0, inode: expected_new_identity.1 };
        require_child_directory_identity(&parent, current_name, old_identity, "installed")?;
        require_child_directory_identity(&swap, new_name, new_identity, "prepared update")?;
        require_child_absent(&swap, backup_name, "rollback")?;
        Ok(Self {
            parent: Some(parent),
            swap: Some(swap),
            swap_identity,
            current_name: current_name.to_os_string(),
            new_name: new_name.to_os_string(),
            backup_name: backup_name.to_os_string(),
            old_identity,
            new_identity,
            armed: true,
        })
    }
}

impl Drop for PreparedMacosReplacement {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if let Some(swap) = &self.swap {
            if require_child_directory_identity(
                swap,
                &self.new_name,
                self.new_identity,
                "prepared update",
            )
            .is_ok()
            {
                let _ = remove_directory_tree_at(swap, &self.new_name, self.new_identity);
            }
            let _ = finalize_swap_container(swap, self.swap_identity);
        }
        self.armed = false;
    }
}

#[derive(Debug)]
enum RecoveryState {
    Activation(ActivationRecovery),
    Rollback(RollbackRecovery),
}

impl RecoveryState {
    fn try_recover(&mut self) -> Result<(), MacosReplaceError> {
        match self {
            Self::Activation(recovery) => recovery.try_recover(),
            Self::Rollback(recovery) => recovery.try_recover(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActivationPhase {
    OldMoved,
    OldRestored,
    PreparedUpdateRemoved,
}

#[derive(Debug)]
struct ActivationRecovery {
    parent: File,
    swap: File,
    swap_identity: DirectoryIdentity,
    current_name: OsString,
    new_name: OsString,
    backup_name: OsString,
    old_identity: DirectoryIdentity,
    new_identity: DirectoryIdentity,
    phase: ActivationPhase,
}

impl ActivationRecovery {
    fn try_recover(&mut self) -> Result<(), MacosReplaceError> {
        if self.phase == ActivationPhase::OldMoved {
            require_child_directory_identity(
                &self.swap,
                &self.backup_name,
                self.old_identity,
                "rollback",
            )?;
            require_child_absent(&self.parent, &self.current_name, "installed app")?;
            require_child_directory_identity(
                &self.swap,
                &self.new_name,
                self.new_identity,
                "prepared update",
            )?;
            rename_at(&self.swap, &self.backup_name, &self.parent, &self.current_name)?;
            self.phase = ActivationPhase::OldRestored;
        }

        if self.phase == ActivationPhase::OldRestored {
            require_child_directory_identity(
                &self.parent,
                &self.current_name,
                self.old_identity,
                "restored",
            )?;
            require_child_directory_identity(
                &self.swap,
                &self.new_name,
                self.new_identity,
                "prepared update",
            )?;
            remove_directory_tree_at(&self.swap, &self.new_name, self.new_identity)?;
            self.phase = ActivationPhase::PreparedUpdateRemoved;
        }

        require_child_directory_identity(
            &self.parent,
            &self.current_name,
            self.old_identity,
            "restored",
        )?;
        finalize_swap_container(&self.swap, self.swap_identity)
    }

    fn into_rollback_recovery(self) -> RollbackRecovery {
        RollbackRecovery {
            parent: self.parent,
            swap: self.swap,
            swap_identity: self.swap_identity,
            current_name: self.current_name,
            backup_name: self.backup_name,
            old_identity: self.old_identity,
            new_identity: self.new_identity,
            phase: RollbackPhase::NewActivated,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RollbackPhase {
    NewActivated,
    NewMovedAside,
    OldRestored,
    FailedUpdateRemoved,
}

#[derive(Debug)]
struct RollbackRecovery {
    parent: File,
    swap: File,
    swap_identity: DirectoryIdentity,
    current_name: OsString,
    backup_name: OsString,
    old_identity: DirectoryIdentity,
    new_identity: DirectoryIdentity,
    phase: RollbackPhase,
}

impl RollbackRecovery {
    fn take_from_transaction(
        transaction: &mut MacosReplacementTransaction,
    ) -> Result<Self, MacosReplaceError> {
        let parent = transaction.parent.take().ok_or_else(|| {
            MacosReplaceError::new("macOS replacement lost its pinned install-parent handle.")
        })?;
        let swap = transaction.swap.take().ok_or_else(|| {
            MacosReplaceError::new("macOS replacement lost its pinned swap-directory handle.")
        })?;
        transaction.active = false;
        Ok(Self {
            parent,
            swap,
            swap_identity: transaction.swap_identity,
            current_name: transaction.current_name.clone(),
            backup_name: transaction.backup_name.clone(),
            old_identity: transaction.old_identity,
            new_identity: transaction.new_identity,
            phase: RollbackPhase::NewActivated,
        })
    }

    fn into_transaction(self) -> MacosReplacementTransaction {
        MacosReplacementTransaction {
            parent: Some(self.parent),
            swap: Some(self.swap),
            swap_identity: self.swap_identity,
            current_name: self.current_name,
            backup_name: self.backup_name,
            old_identity: self.old_identity,
            new_identity: self.new_identity,
            active: true,
        }
    }

    fn try_recover(&mut self) -> Result<(), MacosReplaceError> {
        let failed_name = OsStr::new("failed-new.app");
        if self.phase == RollbackPhase::NewActivated {
            require_child_directory_identity(
                &self.parent,
                &self.current_name,
                self.new_identity,
                "installed update",
            )?;
            require_child_directory_identity(
                &self.swap,
                &self.backup_name,
                self.old_identity,
                "rollback",
            )?;
            require_child_absent(&self.swap, failed_name, "failed-update holding")?;
            rename_at(&self.parent, &self.current_name, &self.swap, failed_name)?;
            self.phase = RollbackPhase::NewMovedAside;
        }

        if self.phase == RollbackPhase::NewMovedAside {
            require_child_directory_identity(
                &self.swap,
                failed_name,
                self.new_identity,
                "failed update",
            )?;
            require_child_directory_identity(
                &self.swap,
                &self.backup_name,
                self.old_identity,
                "rollback",
            )?;
            require_child_absent(&self.parent, &self.current_name, "installed app")?;
            rename_at(&self.swap, &self.backup_name, &self.parent, &self.current_name)?;
            self.phase = RollbackPhase::OldRestored;
        }

        if self.phase == RollbackPhase::OldRestored {
            require_child_directory_identity(
                &self.parent,
                &self.current_name,
                self.old_identity,
                "restored",
            )?;
            require_child_directory_identity(
                &self.swap,
                failed_name,
                self.new_identity,
                "failed update",
            )?;
            remove_directory_tree_at(&self.swap, failed_name, self.new_identity)?;
            self.phase = RollbackPhase::FailedUpdateRemoved;
        }

        require_child_directory_identity(
            &self.parent,
            &self.current_name,
            self.old_identity,
            "restored",
        )?;
        finalize_swap_container(&self.swap, self.swap_identity)
    }
}

#[derive(Debug)]
pub struct MacosReplacementTransaction {
    parent: Option<File>,
    swap: Option<File>,
    swap_identity: DirectoryIdentity,
    current_name: OsString,
    backup_name: OsString,
    old_identity: DirectoryIdentity,
    new_identity: DirectoryIdentity,
    active: bool,
}

impl MacosReplacementTransaction {
    /// Keep the newly-installed app and remove the pinned rollback copy.
    ///
    /// # Errors
    ///
    /// Returns a cleanup error after committing the new app if the pinned rollback tree cannot be removed.
    pub fn commit(mut self) -> Result<(), MacosReplaceError> {
        let installed_check = {
            let parent = self.parent.as_ref().ok_or_else(|| {
                MacosReplaceError::new("macOS replacement lost its pinned install-parent handle.")
            })?;
            require_child_directory_identity(
                parent,
                &self.current_name,
                self.new_identity,
                "installed update",
            )
        };
        if let Err(error) = installed_check {
            let recovery = RollbackRecovery::take_from_transaction(&mut self)?;
            return Err(MacosReplaceError::with_recovery(
                error.to_string(),
                RecoveryState::Rollback(recovery),
            ));
        }

        let backup_directory = {
            let swap = self.swap.as_ref().ok_or_else(|| {
                MacosReplaceError::new("macOS replacement lost its pinned swap-directory handle.")
            })?;
            open_verified_child_directory(swap, &self.backup_name, self.old_identity, "rollback")
        };
        let backup_directory = match backup_directory {
            Ok(directory) => directory,
            Err(error) => {
                let recovery = RollbackRecovery::take_from_transaction(&mut self)?;
                return Err(MacosReplaceError::with_recovery(
                    error.to_string(),
                    RecoveryState::Rollback(recovery),
                ));
            }
        };

        // Commit means keep the new app. Disarm rollback only after both live path identities and the opened
        // rollback directory object are verified, immediately before destructive cleanup of the old bundle.
        self.active = false;
        let swap = self.swap.as_ref().ok_or_else(|| {
            MacosReplaceError::new("macOS replacement lost its pinned swap-directory handle.")
        })?;
        remove_open_directory_tree_at(
            swap,
            &self.backup_name,
            self.old_identity,
            &backup_directory,
        )?;
        finalize_swap_container(swap, self.swap_identity)
    }

    /// Restore the previous app bundle.
    ///
    /// # Errors
    ///
    /// Any failure after rollback starts returns an error that retains the transaction's original pinned
    /// directory handles and exact rollback phase, allowing explicit retry through [`MacosReplaceError::recover`]
    /// or automatic retry when that error is dropped.
    pub fn rollback(mut self) -> Result<(), MacosReplaceError> {
        let mut recovery = RollbackRecovery::take_from_transaction(&mut self)?;
        match recovery.try_recover() {
            Ok(()) => Ok(()),
            Err(error) => Err(MacosReplaceError::with_recovery(
                error.to_string(),
                RecoveryState::Rollback(recovery),
            )),
        }
    }
}

impl Drop for MacosReplacementTransaction {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if let Ok(mut recovery) = RollbackRecovery::take_from_transaction(self) {
            let _ = recovery.try_recover();
        }
        self.active = false;
    }
}

/// Activate a prepared same-filesystem macOS app replacement using pinned directory descriptors.
///
/// The install parent and private swap directory remain open for the entire activation/transaction/recovery
/// lifetime. Every live child stat/rename is descriptor-relative, so renaming or replacing the swap pathname
/// cannot redirect rollback storage.
///
/// # Errors
///
/// Returns before mutation for missing/substituted entries. After the first live rename, failures retain an
/// RAII recovery object inside `MacosReplaceError` so rollback ownership is never returned as an opaque path.
pub fn activate_prepared_macos_replacement(
    mut prepared: PreparedMacosReplacement,
) -> Result<MacosReplacementTransaction, MacosReplaceError> {
    let parent = prepared.parent.take().ok_or_else(|| {
        MacosReplaceError::new("Prepared macOS replacement lost its pinned install-parent handle.")
    })?;
    let swap = prepared.swap.take().ok_or_else(|| {
        MacosReplaceError::new("Prepared macOS replacement lost its pinned swap-directory handle.")
    })?;
    if DirectoryIdentity::from_file(&swap)? != prepared.swap_identity {
        return Err(MacosReplaceError::new("Pinned macOS update swap directory changed identity."));
    }
    require_child_directory_identity(
        &parent,
        &prepared.current_name,
        prepared.old_identity,
        "installed",
    )?;
    require_child_directory_identity(
        &swap,
        &prepared.new_name,
        prepared.new_identity,
        "prepared update",
    )?;
    require_child_absent(&swap, &prepared.backup_name, "rollback")?;

    prepared.armed = false;
    if let Err(error) = rename_at(&parent, &prepared.current_name, &swap, &prepared.backup_name) {
        let _ = remove_directory_tree_at(&swap, &prepared.new_name, prepared.new_identity);
        let _ = finalize_swap_container(&swap, prepared.swap_identity);
        return Err(MacosReplaceError::new(format!(
            "Could not move the current macOS app into rollback storage: {error}"
        )));
    }

    let mut recovery = ActivationRecovery {
        parent,
        swap,
        swap_identity: prepared.swap_identity,
        current_name: std::mem::take(&mut prepared.current_name),
        new_name: std::mem::take(&mut prepared.new_name),
        backup_name: std::mem::take(&mut prepared.backup_name),
        old_identity: prepared.old_identity,
        new_identity: prepared.new_identity,
        phase: ActivationPhase::OldMoved,
    };
    if let Err(error) = require_child_directory_identity(
        &recovery.swap,
        &recovery.backup_name,
        recovery.old_identity,
        "rollback",
    ) {
        return Err(MacosReplaceError::with_recovery(
            format!("{error} Recovery retains pinned directory handles."),
            RecoveryState::Activation(recovery),
        ));
    }

    if let Err(error) =
        rename_at(&recovery.swap, &recovery.new_name, &recovery.parent, &recovery.current_name)
    {
        let recovery_result = recovery.try_recover();
        return Err(match recovery_result {
            Ok(()) => MacosReplaceError::new(format!(
                "Could not move the staged macOS app into place: {error}. The previous app was restored."
            )),
            Err(recovery_error) => MacosReplaceError::with_recovery(
                format!(
                    "Could not move the staged macOS app into place: {error}. Recovery remains available: {recovery_error}"
                ),
                RecoveryState::Activation(recovery),
            ),
        });
    }
    let rollback_recovery = recovery.into_rollback_recovery();
    if let Err(error) = require_child_directory_identity(
        &rollback_recovery.parent,
        &rollback_recovery.current_name,
        rollback_recovery.new_identity,
        "installed update",
    ) {
        return Err(MacosReplaceError::with_recovery(
            format!("{error} Recovery retains pinned directory handles."),
            RecoveryState::Rollback(rollback_recovery),
        ));
    }
    Ok(rollback_recovery.into_transaction())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempTree(PathBuf);

    impl TempTree {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "smu-macos-replace-test-{}-{}",
                std::process::id(),
                TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            assert!(fs::create_dir(&path).is_ok());
            Self(path)
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_marker(app: &Path, value: &str) {
        assert!(fs::create_dir(app).is_ok());
        assert!(fs::write(app.join("marker"), value).is_ok());
    }

    fn identity(path: &Path) -> (u64, u64) {
        let Ok(metadata) = fs::symlink_metadata(path) else {
            return (0, 0);
        };
        (metadata.dev(), metadata.ino())
    }

    #[test]
    fn plain_error_reports_no_recovery_state() {
        let error = MacosReplaceError::new("plain failure");
        assert!(!error.recovery_available());
        assert!(error.recover().is_err());
    }

    fn prepared(tree: &TempTree) -> PreparedMacosReplacement {
        let current = tree.0.join("SMU.app");
        write_marker(&current, "old");
        let swap = tree.0.join(".swap");
        assert!(fs::create_dir(&swap).is_ok());
        let new_app = swap.join("new.app");
        write_marker(&new_app, "new");
        let result = PreparedMacosReplacement::from_paths(
            &swap,
            &current,
            &new_app,
            &swap.join("old.app"),
            identity(&current),
            identity(&new_app),
        );
        assert!(result.is_ok());
        let Ok(result) = result else { unreachable!() };
        result
    }

    #[test]
    fn commit_keeps_new_app_and_empties_private_rollback_tree() {
        let tree = TempTree::new();
        let current = tree.0.join("SMU.app");
        let rollback = tree.0.join(".swap/old.app");
        let transaction = activate_prepared_macos_replacement(prepared(&tree));
        assert!(transaction.is_ok());
        let Ok(transaction) = transaction else { return };
        assert_eq!(fs::read_to_string(current.join("marker")).ok().as_deref(), Some("new"));
        assert!(transaction.commit().is_ok());
        assert!(rollback.is_dir());
        assert!(fs::read_dir(&rollback).is_ok_and(|mut entries| entries.next().is_none()));
    }

    #[test]
    fn commit_identity_failure_retains_recoverable_rollback_state() {
        let tree = TempTree::new();
        let current = tree.0.join("SMU.app");
        let held_new = tree.0.join("held-new.app");
        let transaction = activate_prepared_macos_replacement(prepared(&tree));
        assert!(transaction.is_ok());
        let Ok(transaction) = transaction else { return };
        assert!(fs::rename(&current, &held_new).is_ok());
        write_marker(&current, "substitute");

        let error = transaction.commit();
        assert!(error.is_err());
        let Err(error) = error else { return };
        assert!(error.recovery_available());

        assert!(fs::remove_dir_all(&current).is_ok());
        assert!(fs::rename(&held_new, &current).is_ok());
        assert!(error.recover().is_ok());
        assert_eq!(fs::read_to_string(current.join("marker")).ok().as_deref(), Some("old"));
    }

    #[test]
    fn rollback_and_drop_restore_old_app() {
        let rollback_tree = TempTree::new();
        let rollback_current = rollback_tree.0.join("SMU.app");
        let transaction = activate_prepared_macos_replacement(prepared(&rollback_tree));
        assert!(transaction.is_ok());
        let Ok(transaction) = transaction else { return };
        assert!(transaction.rollback().is_ok());
        assert_eq!(
            fs::read_to_string(rollback_current.join("marker")).ok().as_deref(),
            Some("old")
        );

        let drop_tree = TempTree::new();
        let drop_current = drop_tree.0.join("SMU.app");
        let transaction = activate_prepared_macos_replacement(prepared(&drop_tree));
        assert!(transaction.is_ok());
        drop(transaction);
        assert_eq!(fs::read_to_string(drop_current.join("marker")).ok().as_deref(), Some("old"));
    }

    #[test]
    fn activation_recovery_resumes_after_old_app_was_restored() {
        let tree = TempTree::new();
        let current = tree.0.join("SMU.app");
        let mut prepared = prepared(&tree);
        let parent = prepared.parent.take();
        let swap = prepared.swap.take();
        assert!(parent.is_some());
        assert!(swap.is_some());
        let (Some(parent), Some(swap)) = (parent, swap) else { return };
        prepared.armed = false;

        assert!(rename_at(&parent, &prepared.current_name, &swap, &prepared.backup_name).is_ok());
        assert!(rename_at(&swap, &prepared.backup_name, &parent, &prepared.current_name).is_ok());
        let held_name = OsStr::new("held-new.app");
        assert!(rename_at(&swap, &prepared.new_name, &swap, held_name).is_ok());

        let swap_for_restore = swap.try_clone();
        assert!(swap_for_restore.is_ok());
        let Ok(swap_for_restore) = swap_for_restore else { return };
        let recovery = ActivationRecovery {
            parent,
            swap,
            swap_identity: prepared.swap_identity,
            current_name: prepared.current_name.clone(),
            new_name: prepared.new_name.clone(),
            backup_name: prepared.backup_name.clone(),
            old_identity: prepared.old_identity,
            new_identity: prepared.new_identity,
            phase: ActivationPhase::OldRestored,
        };
        let error = MacosReplaceError::with_recovery(
            "activation cleanup blocked",
            RecoveryState::Activation(recovery),
        );
        assert!(error.recovery_available());
        assert!(
            rename_at(&swap_for_restore, held_name, &swap_for_restore, &prepared.new_name,).is_ok()
        );
        assert!(error.recover().is_ok());
        assert_eq!(fs::read_to_string(current.join("marker")).ok().as_deref(), Some("old"));
    }

    #[test]
    fn rollback_recovery_resumes_after_new_app_was_moved_aside() {
        let tree = TempTree::new();
        let current = tree.0.join("SMU.app");
        let transaction = activate_prepared_macos_replacement(prepared(&tree));
        assert!(transaction.is_ok());
        let Ok(mut transaction) = transaction else { return };
        let recovery = RollbackRecovery::take_from_transaction(&mut transaction);
        assert!(recovery.is_ok());
        let Ok(mut recovery) = recovery else { return };

        let failed_name = OsStr::new("failed-new.app");
        assert!(
            rename_at(&recovery.parent, &recovery.current_name, &recovery.swap, failed_name,)
                .is_ok()
        );
        recovery.phase = RollbackPhase::NewMovedAside;
        write_marker(&current, "blocker");

        let error =
            MacosReplaceError::with_recovery("rollback blocked", RecoveryState::Rollback(recovery));
        assert!(error.recovery_available());
        assert!(fs::remove_dir_all(&current).is_ok());
        assert!(error.recover().is_ok());
        assert_eq!(fs::read_to_string(current.join("marker")).ok().as_deref(), Some("old"));
    }

    #[test]
    fn pinned_swap_handle_survives_swap_path_rename() {
        let tree = TempTree::new();
        let current = tree.0.join("SMU.app");
        let prepared = prepared(&tree);
        let original_swap = tree.0.join(".swap");
        let renamed_swap = tree.0.join(".swap-renamed");
        assert!(fs::rename(&original_swap, &renamed_swap).is_ok());
        assert!(fs::create_dir(&original_swap).is_ok());
        write_marker(&original_swap.join("new.app"), "attacker");

        let transaction = activate_prepared_macos_replacement(prepared);
        assert!(transaction.is_ok());
        let Ok(transaction) = transaction else { return };
        assert_eq!(fs::read_to_string(current.join("marker")).ok().as_deref(), Some("new"));
        assert!(transaction.rollback().is_ok());
        assert_eq!(fs::read_to_string(current.join("marker")).ok().as_deref(), Some("old"));
        assert_eq!(
            fs::read_to_string(original_swap.join("new.app/marker")).ok().as_deref(),
            Some("attacker")
        );
    }

    #[test]
    fn prepared_drop_empties_pinned_swap_after_path_rename_without_touching_substitute() {
        let tree = TempTree::new();
        let prepared = prepared(&tree);
        let original_swap = tree.0.join(".swap");
        let renamed_swap = tree.0.join(".swap-renamed");
        assert!(fs::rename(&original_swap, &renamed_swap).is_ok());
        assert!(fs::create_dir(&original_swap).is_ok());
        write_marker(&original_swap.join("new.app"), "attacker");
        drop(prepared);
        assert_eq!(
            fs::read_to_string(original_swap.join("new.app/marker")).ok().as_deref(),
            Some("attacker")
        );
        let emptied_new = renamed_swap.join("new.app");
        assert!(emptied_new.is_dir());
        assert!(fs::read_dir(&emptied_new).is_ok_and(|mut entries| entries.next().is_none()));
    }
}
