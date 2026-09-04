//! Linux native-input setup orchestration.
//!
//! Device probing lives in `smu-input`. This layer owns package-path discovery, one-shot installer
//! staging/launch, backend reinitialization, and user-facing action status. It never runs the GUI as
//! root and never changes permissions directly.

use crate::linux_input_setup::LinuxInputSetupStatus;
use smu_input::{
    LinuxInputPermissionStatus, linux::EvdevUinputInputDevice, linux_input_permission_status,
};
use std::ffi::CStr;
use std::fs::{self, File, Permissions};
use std::io::{self, Write as _};
use std::os::fd::{FromRawFd as _, RawFd};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

const INSTALLER_RELATIVE: &str = "scripts/install_linux_permissions.sh";
const SHARED_INSTALLER_RELATIVE: &str =
    "usr/share/spencer-macro-utilities/scripts/install_linux_permissions.sh";
const SHARED_DOC_RELATIVE: &str = "usr/share/doc/spencer-macro-utilities/LINUX_SETUP.md";
const SHARED_DATA_DOC_RELATIVE: &str = "usr/share/spencer-macro-utilities/LINUX_SETUP.md";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SetupState {
    status: LinuxInputSetupStatus,
}

#[derive(Debug)]
pub struct LinuxInputSetupService {
    enabled: bool,
    input: Arc<EvdevUinputInputDevice>,
    state: Arc<Mutex<SetupState>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    installer_path: PathBuf,
    docs_path: PathBuf,
    manual_command: String,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl LinuxInputSetupService {
    #[must_use]
    pub fn new(input: Arc<EvdevUinputInputDevice>, enabled: bool) -> Self {
        let executable = executable_directory().unwrap_or_default();
        let app_dirs = app_dir_candidates();
        let current = std::env::current_dir().unwrap_or_default();
        let source_root = source_root();
        let installer_path =
            resolve_installer_path_from(&executable, &app_dirs, &source_root, &current);
        let docs_path = resolve_docs_path_from(&executable, &app_dirs, &source_root, &current);
        let manual_command = build_manual_sudo_command(&installer_path, &current);
        let service = Self {
            enabled,
            input,
            state: Arc::new(Mutex::new(SetupState::default())),
            worker: Mutex::new(None),
            installer_path,
            docs_path,
            manual_command,
        };
        if enabled {
            service.refresh();
        } else {
            let _ = service.input.init();
        }
        service
    }

    pub fn refresh(&self) {
        if !self.enabled {
            return;
        }
        self.reap_worker();
        let permission = linux_input_permission_status();
        let mut action_message = String::new();
        let required = if permission.ready() {
            if self.input.is_initialized() {
                false
            } else {
                match self.input.init() {
                    Ok(()) => false,
                    Err(error) => {
                        action_message = format!("Backend initialization: {error}");
                        true
                    }
                }
            }
        } else {
            if self.input.is_initialized() {
                self.input.shutdown();
            }
            true
        };
        self.publish(&permission, required, action_message, false);
    }

    #[must_use]
    pub fn snapshot(&self) -> LinuxInputSetupStatus {
        if !self.enabled {
            return LinuxInputSetupStatus::default();
        }
        self.reap_worker();
        lock(&self.state).status.clone()
    }

    pub fn install_graphical(&self) {
        if !self.enabled {
            return;
        }
        self.reap_worker();
        if self.worker_running() {
            return;
        }
        if !self.installer_path.is_file() {
            self.set_action_message(format!(
                "Linux permission installer script was not found at {}. The package may be missing the scripts folder.",
                self.installer_path.display()
            ));
            return;
        }
        let installer = self.installer_path.clone();
        let manual_command = self.manual_command.clone();
        let docs_path = self.docs_path.clone();
        let input = Arc::clone(&self.input);
        let state = Arc::clone(&self.state);
        self.set_busy(true);
        let worker = std::thread::spawn(move || {
            let result = run_graphical_installer(&installer);
            let installer_succeeded = result.is_ok();
            let permission = linux_input_permission_status();
            let mut message = match result {
                Ok(()) => "Permission installer completed. Rechecking Linux input permissions..."
                    .to_owned(),
                Err(error) => error,
            };
            let required = if permission.ready() {
                match input.init() {
                    Ok(()) => false,
                    Err(error) => {
                        message = format!(
                            "Installer completed, but backend initialization failed: {error}"
                        );
                        true
                    }
                }
            } else {
                if installer_succeeded {
                    "Installer completed, but this session still cannot access input devices. Log out and back in, reboot, or click Retry after udev finishes applying rules."
                        .clone_into(&mut message);
                }
                true
            };
            let mut guard = lock(&state);
            guard.status = status_from_permission(
                &permission,
                required,
                message,
                false,
                &installer,
                &manual_command,
                &docs_path,
            );
        });
        *lock(&self.worker) = Some(worker);
    }

    pub fn open_terminal_installer(&self) {
        if !self.enabled || self.worker_running() {
            return;
        }
        let message = match launch_terminal_installer(&self.installer_path) {
            Ok(()) => {
                "Terminal installer opened. Complete the prompt, then return here and click Retry permission check.".to_owned()
            }
            Err(error) => format!(
                "SMU could not open a terminal for the Linux permission installer. {error} Run this manually instead: {}",
                self.manual_command
            ),
        };
        self.set_action_message(message);
    }

    pub fn copy_manual_command(&self, window: &sdl3::video::Window) {
        if !self.enabled {
            return;
        }
        let message = match window.subsystem().clipboard().set_clipboard_text(&self.manual_command)
        {
            Ok(()) => format!("Copied: {}", self.manual_command),
            Err(_) => "Could not copy the sudo command to the clipboard.".to_owned(),
        };
        self.set_action_message(message);
    }

    pub fn open_setup_docs(&self) {
        if !self.enabled {
            return;
        }
        let url = file_url(&self.docs_path);
        let message = if url.is_empty() {
            "Linux setup documentation path is unavailable.".to_owned()
        } else {
            match sdl3::url::open_url(&url) {
                Ok(()) => String::new(),
                Err(error) => format!("Could not open Linux setup documentation: {error}"),
            }
        };
        self.set_action_message(message);
    }

    pub fn shutdown(&self) {
        let worker = lock(&self.worker).take();
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }

    fn publish(
        &self,
        permission: &LinuxInputPermissionStatus,
        required: bool,
        action_message: String,
        busy: bool,
    ) {
        let mut status = status_from_permission(
            permission,
            required,
            action_message,
            busy,
            &self.installer_path,
            &self.manual_command,
            &self.docs_path,
        );
        let existing = lock(&self.state).status.clone();
        if status.action_message.is_empty() {
            status.action_message = existing.action_message;
        }
        lock(&self.state).status = status;
    }

    fn set_action_message(&self, message: String) {
        lock(&self.state).status.action_message = message;
    }

    fn set_busy(&self, busy: bool) {
        lock(&self.state).status.busy = busy;
    }

    fn worker_running(&self) -> bool {
        self.reap_worker();
        lock(&self.worker).is_some()
    }

    fn reap_worker(&self) {
        let mut worker = lock(&self.worker);
        if worker.as_ref().is_some_and(JoinHandle::is_finished)
            && let Some(worker) = worker.take()
        {
            let _ = worker.join();
        }
    }
}

fn status_from_permission(
    permission: &LinuxInputPermissionStatus,
    required: bool,
    action_message: String,
    busy: bool,
    installer_path: &Path,
    manual_command: &str,
    docs_path: impl AsRef<Path>,
) -> LinuxInputSetupStatus {
    LinuxInputSetupStatus {
        required,
        summary: permission.summary().to_owned(),
        details: permission.details(),
        installer_path: installer_path.display().to_string(),
        manual_command: manual_command.to_owned(),
        docs_path: docs_path.as_ref().display().to_string(),
        action_message,
        busy,
    }
}

fn executable_directory() -> Option<PathBuf> {
    fs::read_link("/proc/self/exe").ok().and_then(|path| path.parent().map(Path::to_path_buf))
}

fn app_dir_candidates() -> Vec<PathBuf> {
    let mut result = Vec::new();
    for name in ["SMU_APPDIR", "APPDIR"] {
        if let Some(value) = std::env::var_os(name).filter(|value| !value.is_empty()) {
            let path = PathBuf::from(value);
            if !result.contains(&path) {
                result.push(path);
            }
        }
    }
    result
}

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn resolve_installer_path_from(
    executable_dir: &Path,
    app_dirs: &[PathBuf],
    source_root: &Path,
    current_dir: &Path,
) -> PathBuf {
    let packaged = executable_dir.join(INSTALLER_RELATIVE);
    if packaged.is_file() {
        return packaged;
    }
    for app_dir in app_dirs {
        for candidate in [app_dir.join(INSTALLER_RELATIVE), app_dir.join(SHARED_INSTALLER_RELATIVE)]
        {
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    let source = source_root.join(INSTALLER_RELATIVE);
    if source.is_file() {
        return source;
    }
    let cwd = current_dir.join(INSTALLER_RELATIVE);
    if cwd.is_file() {
        return cwd;
    }
    packaged
}

fn resolve_docs_path_from(
    executable_dir: &Path,
    app_dirs: &[PathBuf],
    source_root: &Path,
    current_dir: &Path,
) -> PathBuf {
    let packaged = executable_dir.join("LINUX_SETUP.md");
    if packaged.is_file() {
        return packaged;
    }
    for app_dir in app_dirs {
        for candidate in [
            app_dir.join("LINUX_SETUP.md"),
            app_dir.join(SHARED_DOC_RELATIVE),
            app_dir.join(SHARED_DATA_DOC_RELATIVE),
        ] {
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    let source = source_root.join("LINUX_SETUP.md");
    if source.is_file() {
        return source;
    }
    let cwd = current_dir.join("LINUX_SETUP.md");
    if cwd.is_file() {
        return cwd;
    }
    packaged
}

fn shell_quote(value: &Path) -> String {
    let text = value.to_string_lossy();
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn build_manual_sudo_command(installer_path: &Path, current_dir: &Path) -> String {
    let cwd_script = current_dir.join(INSTALLER_RELATIVE);
    if cwd_script.is_file() {
        return "sudo ./scripts/install_linux_permissions.sh".to_owned();
    }
    format!(
        "SMU_INSTALLER=$(mktemp /tmp/smu-linux-permission-installer.XXXXXX) && cp {} \"$SMU_INSTALLER\" && chmod 700 \"$SMU_INSTALLER\" && sudo /bin/bash \"$SMU_INSTALLER\"; SMU_RC=$?; rm -f \"$SMU_INSTALLER\"; exit $SMU_RC",
        shell_quote(installer_path)
    )
}

fn mkstemp(template: &[u8]) -> io::Result<(File, PathBuf)> {
    let mut buffer = template.to_vec();
    if buffer.last() != Some(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "mkstemp template must be NUL-terminated",
        ));
    }
    // SAFETY: `buffer` is mutable, NUL-terminated, and lives through the call.
    let fd: RawFd = unsafe { libc::mkstemp(buffer.as_mut_ptr().cast()) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful `mkstemp` returns a fresh owned descriptor.
    let file = unsafe { File::from_raw_fd(fd) };
    // SAFETY: successful `mkstemp` leaves a NUL-terminated path in `buffer`.
    let path = unsafe { CStr::from_ptr(buffer.as_ptr().cast()) };
    Ok((file, PathBuf::from(path.to_string_lossy().into_owned())))
}

struct StagedInstaller(PathBuf);

impl Drop for StagedInstaller {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

struct TerminalInstallerArtifacts {
    launcher: StagedInstaller,
    installer: StagedInstaller,
}

fn stage_installer(source: &Path) -> Result<StagedInstaller, String> {
    if !source.is_file() {
        return Err(format!(
            "Linux permission installer script was not found at {}.",
            source.display()
        ));
    }
    let (mut destination, path) = mkstemp(b"/tmp/smu-linux-permission-installer-copy-XXXXXX\0")
        .map_err(|error| format!("Could not create a temporary installer copy: {error}"))?;
    destination
        .set_permissions(Permissions::from_mode(0o700))
        .map_err(|error| format!("Could not protect temporary installer copy: {error}"))?;
    let mut source_file = File::open(source)
        .map_err(|error| format!("Could not open bundled installer script: {error}"))?;
    io::copy(&mut source_file, &mut destination)
        .map_err(|error| format!("Could not stage Linux permission installer: {error}"))?;
    destination
        .flush()
        .and_then(|()| destination.sync_all())
        .map_err(|error| format!("Could not finish staging Linux permission installer: {error}"))?;
    Ok(StagedInstaller(path))
}

fn run_graphical_installer(source: &Path) -> Result<(), String> {
    let staged = stage_installer(source)?;
    let status = Command::new("pkexec")
        .arg("--disable-internal-agent")
        .arg("/bin/bash")
        .arg(&staged.0)
        .status()
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                "Graphical authentication did not start because pkexec was not found. Install polkit or use the terminal installer.".to_owned()
            } else {
                format!("Graphical authentication could not start: {error}")
            }
        })?;
    if status.success() {
        Ok(())
    } else {
        Err("Graphical authentication did not start, or permission was denied. Your desktop may not be running a graphical polkit agent. Try the terminal installer.".to_owned())
    }
}

fn create_terminal_script(installer: &Path) -> Result<TerminalInstallerArtifacts, String> {
    let staged = stage_installer(installer)?;
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let template = runtime_dir.join("smu-linux-permission-terminal-XXXXXX");
    let mut bytes = template.as_os_str().as_encoded_bytes().to_vec();
    bytes.push(0);
    let (mut file, path) = mkstemp(&bytes)
        .map_err(|error| format!("Could not create a temporary installer launcher: {error}"))?;
    file.set_permissions(Permissions::from_mode(0o700))
        .map_err(|error| format!("Could not protect the terminal installer launcher: {error}"))?;
    let installer_path = shell_quote(&staged.0);
    let contents = format!(
        "#!/usr/bin/env bash\nINSTALLER_COPY={installer_path}\ncleanup() {{ rm -f -- \"$0\" \"$INSTALLER_COPY\"; }}\ntrap cleanup EXIT\necho \"Spencer Macro Utilities needs permission to access Linux input devices.\"\necho\nTARGET_USER=\"$(id -un)\"\nUSER_GROUPS=\" $(id -nG) \"\nif command -v sudo >/dev/null 2>&1 && [[ \"$USER_GROUPS\" == *\" sudo \"* || \"$USER_GROUPS\" == *\" wheel \"* || \"$USER_GROUPS\" == *\" admin \"* ]]; then\n  sudo env SMU_TARGET_USER=\"$TARGET_USER\" /bin/bash \"$INSTALLER_COPY\"\n  status=$?\nelif command -v su >/dev/null 2>&1; then\n  echo \"This account is not configured for sudo; using the system administrator password instead.\"\n  su -c \"SMU_TARGET_USER=$(printf '%q' \"$TARGET_USER\") /bin/bash $(printf '%q' \"$INSTALLER_COPY\")\" root\n  status=$?\nelse\n  echo \"Neither sudo nor su is available to run the installer as administrator.\" >&2\n  status=1\nfi\necho\nif [[ \"$status\" -eq 0 ]]; then\n  echo \"Installer completed. Return to SMU and click Retry permission check.\"\nelse\n  echo \"Installer failed with exit code $status.\"\nfi\necho\nprintf \"Press Enter to close...\"\nread -r _\nexit \"$status\"\n"
    );
    file.write_all(contents.as_bytes())
        .and_then(|()| file.flush())
        .map_err(|error| format!("Could not write the terminal installer launcher: {error}"))?;
    Ok(TerminalInstallerArtifacts { launcher: StagedInstaller(path), installer: staged })
}

fn launch_terminal_installer(installer: &Path) -> Result<(), String> {
    if !installer.is_file() {
        return Err(format!(
            "Linux permission installer script was not found at {}.",
            installer.display()
        ));
    }
    let artifacts = create_terminal_script(installer)?;
    let launcher_path = artifacts.launcher.0.clone();
    if !artifacts.installer.0.is_file() {
        return Err(
            "The staged Linux permission installer disappeared before terminal launch.".to_owned()
        );
    }
    let mut candidates: Vec<Vec<String>> = vec![
        vec!["xdg-terminal-exec".into(), "/bin/bash".into(), launcher_path.display().to_string()],
        vec![
            "gnome-terminal".into(),
            "--".into(),
            "/bin/bash".into(),
            launcher_path.display().to_string(),
        ],
        vec!["kgx".into(), "--".into(), "/bin/bash".into(), launcher_path.display().to_string()],
        vec![
            "konsole".into(),
            "-e".into(),
            "/bin/bash".into(),
            launcher_path.display().to_string(),
        ],
        vec![
            "xfce4-terminal".into(),
            "--command".into(),
            format!("/bin/bash {}", shell_quote(&launcher_path)),
        ],
        vec![
            "mate-terminal".into(),
            "--".into(),
            "/bin/bash".into(),
            launcher_path.display().to_string(),
        ],
        vec![
            "lxterminal".into(),
            "-e".into(),
            "/bin/bash".into(),
            launcher_path.display().to_string(),
        ],
        vec!["kitty".into(), "/bin/bash".into(), launcher_path.display().to_string()],
        vec![
            "alacritty".into(),
            "-e".into(),
            "/bin/bash".into(),
            launcher_path.display().to_string(),
        ],
        vec!["foot".into(), "/bin/bash".into(), launcher_path.display().to_string()],
        vec![
            "wezterm".into(),
            "start".into(),
            "--".into(),
            "/bin/bash".into(),
            launcher_path.display().to_string(),
        ],
        vec!["xterm".into(), "-e".into(), "/bin/bash".into(), launcher_path.display().to_string()],
    ];
    if let Some(terminal) = std::env::var_os("TERMINAL").filter(|value| !value.is_empty()) {
        let mut command = terminal
            .to_string_lossy()
            .split_ascii_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !command.is_empty() {
            command.push("/bin/bash".to_owned());
            command.push(launcher_path.display().to_string());
            candidates.insert(1, command);
        }
    }
    for candidate in candidates {
        let Some((program, args)) = candidate.split_first() else {
            continue;
        };
        let spawned = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match spawned {
            Ok(_) => {
                // The launcher deletes itself and the staged installer after the terminal flow exits.
                std::mem::forget(artifacts);
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Failed to launch terminal installer: {error}")),
        }
    }
    Err("No supported terminal emulator was found in PATH.".to_owned())
}

fn file_url(path: &Path) -> String {
    if path.as_os_str().is_empty() {
        return String::new();
    }
    format!("file://{}", path.display().to_string().replace(' ', "%20"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let index = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("smu-input-setup-service-{}-{index}", std::process::id()));
            fs::create_dir_all(&path).expect("test root");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn installer_resolution_matches_cpp_priority() {
        let root = TestDirectory::new();
        let executable = root.0.join("exe");
        let app = root.0.join("app");
        let source = root.0.join("source");
        let current = root.0.join("cwd");
        for path in [&executable, &app, &source, &current] {
            fs::create_dir_all(path.join("scripts")).expect("candidate dir");
        }
        fs::write(app.join(INSTALLER_RELATIVE), b"app").expect("app installer");
        fs::write(source.join(INSTALLER_RELATIVE), b"source").expect("source installer");
        assert_eq!(
            resolve_installer_path_from(&executable, std::slice::from_ref(&app), &source, &current),
            app.join(INSTALLER_RELATIVE)
        );
        fs::write(executable.join(INSTALLER_RELATIVE), b"exe").expect("exe installer");
        assert_eq!(
            resolve_installer_path_from(&executable, &[app], &source, &current),
            executable.join(INSTALLER_RELATIVE)
        );
    }

    #[test]
    fn manual_command_uses_short_cwd_form_when_available() {
        let root = TestDirectory::new();
        fs::create_dir_all(root.0.join("scripts")).expect("scripts dir");
        let installer = root.0.join(INSTALLER_RELATIVE);
        fs::write(&installer, b"fixture").expect("installer");
        assert_eq!(
            build_manual_sudo_command(&installer, &root.0),
            "sudo ./scripts/install_linux_permissions.sh"
        );
    }

    #[test]
    fn terminal_artifacts_cleanup_when_not_handed_to_terminal() {
        let root = TestDirectory::new();
        let installer = root.0.join("installer.sh");
        fs::write(&installer, b"#!/bin/bash\nexit 0\n").expect("installer");
        let artifacts = create_terminal_script(&installer).expect("terminal artifacts");
        let launcher = artifacts.launcher.0.clone();
        let staged = artifacts.installer.0.clone();
        assert!(launcher.is_file());
        assert!(staged.is_file());
        drop(artifacts);
        assert!(!launcher.exists());
        assert!(!staged.exists());
    }

    #[test]
    fn shell_quote_preserves_single_quotes() {
        assert_eq!(shell_quote(Path::new("/tmp/a'b")), "'/tmp/a'\\''b'");
    }
}
