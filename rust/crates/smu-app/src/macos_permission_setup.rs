use std::path::{Path, PathBuf};

pub const TITLE: &str = "macOS Input Permissions";
const SMU_BUNDLE_IDENTIFIER: &str = "com.spencer0187.smu";
const PERMISSION_REPAIR_MARKER: &str = "macos-permissions-may-need-repair";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MacosPermissionSetupStatus {
    pub restart_required: bool,
    pub accessibility_granted: bool,
    pub screen_recording_granted: bool,
    pub summary: String,
    pub details: String,
    pub action_message: String,
}

impl MacosPermissionSetupStatus {
    #[must_use]
    pub const fn permissions_required(&self) -> bool {
        !(self.accessibility_granted && self.screen_recording_granted)
    }
}

fn status_from_permissions(
    accessibility_granted: bool,
    screen_recording_granted: bool,
    permission_repair_recommended: bool,
    action_message: String,
) -> MacosPermissionSetupStatus {
    let permissions_required = !(accessibility_granted && screen_recording_granted);
    let summary = if permissions_required {
        "Grant Accessibility and Screen Recording, then restart SMU.".to_owned()
    } else {
        "macOS permissions are configured.".to_owned()
    };
    let mut details = format!(
        "Accessibility for macro input: {}\nScreen Recording for Lua pixel reads: {}",
        if accessibility_granted { "Granted" } else { "Missing" },
        if screen_recording_granted { "Granted" } else { "Missing" }
    );
    if permissions_required {
        details.push_str(
            "\n\nIf System Settings already shows SMU enabled but this still says Missing, use Reset macOS Permission Entries below, approve the current app again, and restart SMU.",
        );
    }
    if permission_repair_recommended {
        details.push_str(
            "\n\nSMU was just updated. If permissions do not work after this launch, reset the macOS permission entries and approve SMU again.",
        );
    }
    MacosPermissionSetupStatus {
        restart_required: false,
        accessibility_granted,
        screen_recording_granted,
        summary,
        details,
        action_message,
    }
}

fn permission_repair_marker_path(home: &Path) -> PathBuf {
    home.join("Library/Application Support/Spencer Macro Utilities").join(PERMISSION_REPAIR_MARKER)
}

fn app_bundle_from_executable(executable: &Path) -> Option<PathBuf> {
    let contents = executable.parent()?.parent()?;
    let bundle = contents.parent()?;
    (bundle.extension().and_then(|value| value.to_str()) == Some("app"))
        .then(|| bundle.to_path_buf())
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{
        MacosPermissionSetupStatus, Path, PathBuf, SMU_BUNDLE_IDENTIFIER,
        app_bundle_from_executable, permission_repair_marker_path, status_from_permissions,
    };
    use smu_display::macos::{
        accessibility_trusted, request_accessibility_permission,
        request_screen_recording_permission, screen_recording_trusted,
    };
    use std::fs;
    use std::os::unix::process::ExitStatusExt;
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, MutexGuard};

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        match mutex.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn consume_permission_repair_marker() -> bool {
        let Some(home) = std::env::var_os("HOME") else {
            return false;
        };
        let marker = permission_repair_marker_path(Path::new(&home));
        if !marker.exists() {
            return false;
        }
        // Match C++: once the marker was observed, show the repair advisory for this launch even if
        // removing the marker subsequently fails because Application Support became unwritable.
        let _ = fs::remove_file(marker);
        true
    }

    fn open_url(url: &str) -> bool {
        Command::new("/usr/bin/open").arg(url).status().is_ok_and(|status| status.success())
    }

    fn open_settings_pane(pane: &str) -> bool {
        let pane_url = format!("x-apple.systempreferences:com.apple.preference.security?{pane}");
        open_url(&pane_url)
            || open_url("x-apple.systempreferences:com.apple.preference.security?Privacy")
    }

    fn open_accessibility_settings() -> bool {
        open_settings_pane("Privacy_Accessibility")
    }

    fn open_screen_recording_settings() -> bool {
        open_settings_pane("Privacy_ScreenCapture")
    }

    fn open_privacy_security_settings() -> bool {
        open_settings_pane("Privacy")
    }

    fn reset_tcc_service(service: &str) -> Result<(), String> {
        let status = Command::new("/usr/bin/tccutil")
            .args(["reset", service, SMU_BUNDLE_IDENTIFIER])
            .status()
            .map_err(|error| format!("Failed to launch tccutil for {service}: {error}"))?;
        if status.success() {
            return Ok(());
        }
        if let Some(code) = status.code() {
            return Err(format!("tccutil reset {service} exited with code {code}."));
        }
        if let Some(signal) = status.signal() {
            return Err(format!("tccutil reset {service} was terminated by signal {signal}."));
        }
        Err(format!("tccutil reset {service} did not complete successfully."))
    }

    fn current_app_bundle_path() -> Result<PathBuf, String> {
        let executable = std::env::current_exe()
            .map_err(|error| format!("Could not resolve current macOS executable: {error}"))?;
        let executable = fs::canonicalize(executable)
            .map_err(|error| format!("Could not canonicalize current macOS executable: {error}"))?;
        app_bundle_from_executable(&executable).ok_or_else(|| {
            "Could not resolve current macOS app bundle path for restart.".to_owned()
        })
    }

    fn relaunch_current_app() -> Result<(), String> {
        let bundle = current_app_bundle_path()?;
        let status =
            Command::new("/usr/bin/open").arg("-n").arg(bundle).status().map_err(|error| {
                format!("Could not relaunch SMU through /usr/bin/open: {error}")
            })?;
        if status.success() {
            return Ok(());
        }
        if let Some(code) = status.code() {
            return Err(format!("/usr/bin/open -n exited with code {code}."));
        }
        if let Some(signal) = status.signal() {
            return Err(format!("/usr/bin/open -n was terminated by signal {signal}."));
        }
        Err("/usr/bin/open -n did not complete successfully.".to_owned())
    }

    #[derive(Debug)]
    pub struct MacosPermissionSetupService {
        status: Mutex<MacosPermissionSetupStatus>,
        permission_repair_recommended: bool,
        restart_required: AtomicBool,
        restart_requested: AtomicBool,
    }

    impl MacosPermissionSetupService {
        #[must_use]
        pub fn new() -> Self {
            let permission_repair_recommended = consume_permission_repair_marker();
            let status = status_from_permissions(
                accessibility_trusted(),
                screen_recording_trusted(),
                permission_repair_recommended,
                String::new(),
            );
            Self {
                status: Mutex::new(status),
                permission_repair_recommended,
                restart_required: AtomicBool::new(false),
                restart_requested: AtomicBool::new(false),
            }
        }

        fn refresh_with_message(&self, action_message: String) {
            *lock(&self.status) = status_from_permissions(
                accessibility_trusted(),
                screen_recording_trusted(),
                self.permission_repair_recommended,
                action_message,
            );
        }

        #[must_use]
        pub fn prepare(&self) -> MacosPermissionSetupStatus {
            let mut status = lock(&self.status).clone();
            status.restart_required = self.restart_required.load(Ordering::Acquire);
            status
        }

        pub fn require_restart_for_runtime(&self) {
            self.restart_required.store(true, Ordering::Release);
        }

        pub fn request_accessibility(&self) {
            let trusted = request_accessibility_permission();
            let now_trusted = trusted || accessibility_trusted();
            let message = if now_trusted {
                "Accessibility permission is granted. Restart and check permissions.".to_owned()
            } else if open_accessibility_settings() {
                "Accessibility permission requested. Approve SMU, then restart and check permissions."
                    .to_owned()
            } else {
                "macOS could not open Accessibility settings automatically.".to_owned()
            };
            self.refresh_with_message(message);
        }

        pub fn request_screen_recording(&self) {
            if screen_recording_trusted() {
                self.refresh_with_message(
                    "Screen Recording permission is already granted.".to_owned(),
                );
                return;
            }
            let trusted = request_screen_recording_permission();
            let now_trusted = trusted || screen_recording_trusted();
            let message = if now_trusted {
                "Screen Recording permission is granted. Restart and check permissions.".to_owned()
            } else if open_screen_recording_settings() {
                "Screen Recording settings opened. Approve SMU, then restart and check permissions."
                    .to_owned()
            } else {
                "macOS could not open Screen Recording settings automatically.".to_owned()
            };
            self.refresh_with_message(message);
        }

        pub fn reset_permission_entries(&self) {
            let accessibility = reset_tcc_service("Accessibility");
            let screen_recording = reset_tcc_service("ScreenCapture");
            let message = if accessibility.is_ok() && screen_recording.is_ok() {
                let _ = open_privacy_security_settings();
                "macOS privacy entries for SMU were reset. Re-enable Accessibility and Screen Recording, then restart SMU."
                    .to_owned()
            } else {
                let mut message = "Could not reset every macOS privacy entry.".to_owned();
                if let Err(error) = accessibility {
                    message.push_str(" Accessibility: ");
                    message.push_str(&error);
                }
                if let Err(error) = screen_recording {
                    message.push_str(" Screen Recording: ");
                    message.push_str(&error);
                }
                message
            };
            self.refresh_with_message(message);
        }

        pub fn restart_and_check(&self) {
            match relaunch_current_app() {
                Ok(()) => {
                    self.refresh_with_message("Restarting SMU...".to_owned());
                    self.restart_requested.store(true, Ordering::Release);
                }
                Err(error) => {
                    self.refresh_with_message(format!(
                        "Could not restart SMU automatically. Quit and reopen the app. {error}"
                    ));
                }
            }
        }

        #[must_use]
        pub fn restart_requested(&self) -> bool {
            self.restart_requested.load(Ordering::Acquire)
        }
    }

    impl Default for MacosPermissionSetupService {
        fn default() -> Self {
            Self::new()
        }
    }
}

#[cfg(target_os = "macos")]
pub use platform::MacosPermissionSetupService;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_cpp_permission_contract() {
        assert_eq!(TITLE, "macOS Input Permissions");
        assert_eq!(SMU_BUNDLE_IDENTIFIER, "com.spencer0187.smu");
    }

    #[test]
    fn status_matches_cpp_permission_summary_and_repair_details() {
        let status = status_from_permissions(false, true, true, "action".to_owned());
        assert!(status.permissions_required());
        assert!(!status.accessibility_granted);
        assert!(status.screen_recording_granted);
        assert_eq!(status.summary, "Grant Accessibility and Screen Recording, then restart SMU.");
        assert!(status.details.contains("Accessibility for macro input: Missing"));
        assert!(status.details.contains("Screen Recording for Lua pixel reads: Granted"));
        assert!(status.details.contains("SMU was just updated."));
        assert_eq!(status.action_message, "action");
    }

    #[test]
    fn bundle_path_requires_contents_macos_shape() {
        let executable = Path::new(
            "/Applications/Spencer Macro Utilities.app/Contents/MacOS/Spencer-Macro-Utilities",
        );
        assert_eq!(
            app_bundle_from_executable(executable),
            Some(PathBuf::from("/Applications/Spencer Macro Utilities.app"))
        );
        assert_eq!(app_bundle_from_executable(Path::new("/tmp/smu")), None);
    }

    #[test]
    fn repair_marker_path_matches_macos_updater_helper_contract() {
        assert_eq!(
            permission_repair_marker_path(Path::new("/Users/test")),
            PathBuf::from(
                "/Users/test/Library/Application Support/Spencer Macro Utilities/macos-permissions-may-need-repair"
            )
        );
    }
}
