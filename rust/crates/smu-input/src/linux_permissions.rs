//! Read-only Linux native-input permission diagnostics.
//!
//! This mirrors the C++ `/dev/input/event*` and `/dev/uinput` preflight. The probe never changes
//! permissions or creates devices; installer orchestration belongs to the application layer.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinuxInputPermissionStatus {
    pub input_directory_available: bool,
    pub can_read_input_events: bool,
    pub can_open_uinput: bool,
    pub readable_event_device_count: usize,
    pub input_events_message: String,
    pub uinput_message: String,
}

impl LinuxInputPermissionStatus {
    #[must_use]
    pub const fn ready(&self) -> bool {
        self.can_read_input_events && self.can_open_uinput
    }

    #[must_use]
    pub const fn summary(&self) -> &'static str {
        if self.ready() {
            "Linux input permissions are configured."
        } else {
            "Linux input permissions are not configured yet."
        }
    }

    #[must_use]
    pub fn details(&self) -> String {
        format!("Input events: {}\nuinput: {}", self.input_events_message, self.uinput_message)
    }
}

#[must_use]
pub fn linux_input_permission_status() -> LinuxInputPermissionStatus {
    permission_status_from_paths(Path::new("/dev/input"), Path::new("/dev/uinput"))
}

fn permission_status_from_paths(input_dir: &Path, uinput: &Path) -> LinuxInputPermissionStatus {
    let mut status = LinuxInputPermissionStatus::default();
    match fs::metadata(input_dir) {
        Err(error) => {
            status.input_events_message = format!("Cannot access {}: {error}", input_dir.display());
        }
        Ok(metadata) if !metadata.is_dir() => {
            status.input_events_message =
                format!("{} exists but is not a directory.", input_dir.display());
        }
        Ok(_) => {
            status.input_directory_available = true;
            inspect_event_devices(input_dir, &mut status);
        }
    }
    inspect_uinput(uinput, &mut status);
    status
}

fn inspect_event_devices(input_dir: &Path, status: &mut LinuxInputPermissionStatus) {
    let devices = enumerate_event_devices(input_dir);
    if devices.is_empty() {
        status.input_events_message =
            format!("No {}/event* devices were found.", input_dir.display());
    }

    let mut last_open_error = String::new();
    for path in devices {
        match open_nonblocking_read(&path) {
            Ok(mut file) => {
                if can_read_nonblocking(&mut file) {
                    status.readable_event_device_count += 1;
                } else {
                    last_open_error = format!("Cannot read {}", path.display());
                }
            }
            Err(error) => {
                last_open_error = format!("Cannot open {}: {error}", path.display());
            }
        }
    }

    status.can_read_input_events = status.readable_event_device_count > 0;
    if status.can_read_input_events {
        status.input_events_message = format!(
            "Readable {}/event* devices: {}",
            input_dir.display(),
            status.readable_event_device_count
        );
    } else if status.input_events_message.is_empty() {
        status.input_events_message = if last_open_error.is_empty() {
            format!("No readable {}/event* devices were found.", input_dir.display())
        } else {
            last_open_error
        };
    }
}

fn inspect_uinput(path: &Path, status: &mut LinuxInputPermissionStatus) {
    match OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC).open(path)
    {
        Err(error) => {
            status.uinput_message = format!("Cannot open {} for writing: {error}", path.display());
        }
        Ok(mut file) => match file.write(&[]) {
            Ok(0) => {
                status.can_open_uinput = true;
                status.uinput_message = format!("{} is writable.", path.display());
            }
            Ok(_) => {
                status.uinput_message = format!("Cannot write to {}", path.display());
            }
            Err(error) => {
                status.uinput_message = format!("Cannot write to {}: {error}", path.display());
            }
        },
    }
}

fn enumerate_event_devices(input_dir: &Path) -> Vec<PathBuf> {
    let mut devices = fs::read_dir(input_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("event"))
                .then(|| entry.path())
        })
        .collect::<Vec<_>>();
    devices.sort();
    devices
}

fn open_nonblocking_read(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC).open(path)
}

fn can_read_nonblocking(file: &mut File) -> bool {
    let mut buffer = [0_u8; 64];
    match file.read(&mut buffer) {
        Ok(bytes) => bytes > 0,
        Err(error) => matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted),
    }
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
                .join(format!("smu-input-permission-{}-{index}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn readable_event_and_writable_uinput_report_ready() {
        let root = TestDirectory::new();
        let input = root.0.join("input");
        fs::create_dir(&input).expect("input dir");
        fs::write(input.join("event9"), b"event").expect("event fixture");
        fs::write(input.join("mouse0"), b"ignored").expect("non-event fixture");
        let uinput = root.0.join("uinput");
        fs::write(&uinput, b"").expect("uinput fixture");

        let status = permission_status_from_paths(&input, &uinput);
        assert!(status.input_directory_available);
        assert_eq!(status.readable_event_device_count, 1);
        assert!(status.can_read_input_events);
        assert!(status.can_open_uinput);
        assert!(status.ready());
        assert!(status.input_events_message.contains("Readable"));
        assert!(status.uinput_message.ends_with("is writable."));
    }

    #[test]
    fn missing_paths_report_both_failures() {
        let root = TestDirectory::new();
        let input = root.0.join("missing-input");
        let uinput = root.0.join("missing-uinput");
        let status = permission_status_from_paths(&input, &uinput);
        assert!(!status.ready());
        assert!(!status.input_directory_available);
        assert!(status.input_events_message.starts_with("Cannot access"));
        assert!(status.uinput_message.starts_with("Cannot open"));
    }

    #[test]
    fn empty_input_directory_matches_cpp_no_devices_message() {
        let root = TestDirectory::new();
        let input = root.0.join("input");
        fs::create_dir(&input).expect("input dir");
        let uinput = root.0.join("uinput");
        fs::write(&uinput, b"").expect("uinput fixture");
        let status = permission_status_from_paths(&input, &uinput);
        assert!(!status.can_read_input_events);
        assert!(status.input_events_message.contains("No "));
        assert!(status.input_events_message.contains("/event* devices were found."));
    }
}
