use smu_scripting::ScriptLogSink;
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

const MAX_LOG_ENTRIES: usize = 512;
const DEFAULT_LOG_FILENAME: &str = "SMC.log";
#[cfg(not(target_os = "windows"))]
const FALLBACK_TIMESTAMP: &str = "1970-01-01 00:00:00";

static APP_SCRIPT_LOGGER: OnceLock<Arc<AppScriptLogger>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AppLogLevel {
    Info,
    Warning,
}

impl AppLogLevel {
    const fn name(self) -> &'static [u8] {
        match self {
            Self::Info => b"INFO",
            Self::Warning => b"WARN",
        }
    }
}

#[derive(Debug, Default)]
struct LogState {
    entries: VecDeque<Vec<u8>>,
    warnings: VecDeque<Vec<u8>>,
}

#[derive(Debug)]
pub struct AppScriptLogger {
    state: Mutex<LogState>,
    console_logging_enabled: bool,
    file_logging_enabled: bool,
    file_path: PathBuf,
}

impl Default for AppScriptLogger {
    fn default() -> Self {
        let env_debug = debug_logging_enabled();
        Self {
            state: Mutex::new(LogState {
                entries: VecDeque::with_capacity(MAX_LOG_ENTRIES),
                warnings: VecDeque::with_capacity(MAX_LOG_ENTRIES),
            }),
            console_logging_enabled: cfg!(debug_assertions) || env_debug,
            file_logging_enabled: env_debug,
            file_path: default_log_path(),
        }
    }
}

pub fn shared() -> Arc<AppScriptLogger> {
    Arc::clone(APP_SCRIPT_LOGGER.get_or_init(|| Arc::new(AppScriptLogger::default())))
}

pub fn script_sink() -> Arc<dyn ScriptLogSink> {
    shared()
}

pub fn drain_warnings() -> Vec<String> {
    shared().drain_warnings()
}

impl AppScriptLogger {
    #[cfg(test)]
    fn for_test(
        console_logging_enabled: bool,
        file_logging_enabled: bool,
        file_path: PathBuf,
    ) -> Self {
        Self {
            state: Mutex::new(LogState {
                entries: VecDeque::with_capacity(MAX_LOG_ENTRIES),
                warnings: VecDeque::with_capacity(MAX_LOG_ENTRIES),
            }),
            console_logging_enabled,
            file_logging_enabled,
            file_path,
        }
    }

    pub fn drain_warnings(&self) -> Vec<String> {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .warnings
            .drain(..)
            .map(|message| String::from_utf8_lossy(&message).into_owned())
            .collect()
    }

    fn log(&self, level: AppLogLevel, message: &[u8]) {
        let formatted = format_entry(level, message);
        self.retain_and_append_debug_file(level, message, &formatted);

        if self.console_logging_enabled {
            let stdout = io::stdout();
            let mut stdout = stdout.lock();
            let _ = write_formatted_line(&mut stdout, &formatted);
        }
        write_windows_debug_line(&formatted);
    }

    #[cfg(test)]
    fn snapshot(&self) -> Vec<Vec<u8>> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entries
            .iter()
            .cloned()
            .collect()
    }

    #[allow(clippy::significant_drop_tightening)]
    fn retain_and_append_debug_file(&self, level: AppLogLevel, message: &[u8], formatted: &[u8]) {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.entries.len() >= MAX_LOG_ENTRIES {
            state.entries.pop_front();
        }
        state.entries.push_back(message.to_vec());
        if level == AppLogLevel::Warning {
            if state.warnings.len() >= MAX_LOG_ENTRIES {
                state.warnings.pop_front();
            }
            state.warnings.push_back(message.to_vec());
        }

        // Match the C++ logger's single-mutex ordering: the retained history and optional SMC.log append
        // are committed under one lock, so concurrent scripts cannot reorder the file relative to the ring.
        if self.file_logging_enabled
            && let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&self.file_path)
        {
            let _ = write_formatted_line(&mut file, formatted);
        }
    }
}

impl ScriptLogSink for AppScriptLogger {
    fn log_info(&self, message: &[u8]) {
        self.log(AppLogLevel::Info, message);
    }

    fn log_warning(&self, message: &[u8]) {
        self.log(AppLogLevel::Warning, message);
    }
}

fn debug_logging_enabled() -> bool {
    ["DEBUG", "SMU_DEBUG"]
        .into_iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| value == "1"))
}

fn default_log_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|executable| {
            executable.parent().map(|directory| directory.join(DEFAULT_LOG_FILENAME))
        })
        .unwrap_or_else(|| PathBuf::from(DEFAULT_LOG_FILENAME))
}

fn format_entry(level: AppLogLevel, message: &[u8]) -> Vec<u8> {
    let timestamp = local_timestamp();
    let mut formatted =
        Vec::with_capacity(timestamp.len() + b"[] [] ".len() + level.name().len() + message.len());
    formatted.push(b'[');
    formatted.extend_from_slice(timestamp.as_bytes());
    formatted.extend_from_slice(b"] [");
    formatted.extend_from_slice(level.name());
    formatted.extend_from_slice(b"] ");
    formatted.extend_from_slice(message);
    formatted
}

fn write_formatted_line(writer: &mut impl Write, formatted: &[u8]) -> io::Result<()> {
    writer.write_all(formatted)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn local_timestamp() -> String {
    use std::mem::MaybeUninit;
    use std::time::{SystemTime, UNIX_EPOCH};

    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let Ok(raw_time) = libc::time_t::try_from(seconds) else {
        return FALLBACK_TIMESTAMP.to_owned();
    };
    let mut local = MaybeUninit::<libc::tm>::zeroed();
    // SAFETY: `raw_time` and `local` are valid for this synchronous libc call. A non-null return initializes
    // `local` exactly as required by localtime_r.
    if unsafe { libc::localtime_r(&raw const raw_time, local.as_mut_ptr()) }.is_null() {
        return FALLBACK_TIMESTAMP.to_owned();
    }
    // SAFETY: localtime_r returned non-null above, so it initialized the tm value.
    let local = unsafe { local.assume_init() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        local.tm_year + 1900,
        local.tm_mon + 1,
        local.tm_mday,
        local.tm_hour,
        local.tm_min,
        local.tm_sec
    )
}

#[cfg(target_os = "windows")]
fn local_timestamp() -> String {
    use std::mem::MaybeUninit;
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;

    let mut local = MaybeUninit::<SYSTEMTIME>::zeroed();
    // SAFETY: `local` points to writable storage for a SYSTEMTIME and GetLocalTime initializes it synchronously.
    unsafe { GetLocalTime(local.as_mut_ptr()) };
    // SAFETY: GetLocalTime initializes the supplied SYSTEMTIME before returning.
    let local = unsafe { local.assume_init() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute, local.wSecond
    )
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn local_timestamp() -> String {
    FALLBACK_TIMESTAMP.to_owned()
}

#[cfg(target_os = "windows")]
fn write_windows_debug_line(formatted: &[u8]) {
    use windows_sys::Win32::System::Diagnostics::Debug::OutputDebugStringA;

    let visible = formatted.split(|byte| *byte == 0).next().unwrap_or_default();
    let mut output = Vec::with_capacity(visible.len() + 2);
    output.extend_from_slice(visible);
    output.push(b'\n');
    output.push(0);
    // SAFETY: `output` is NUL-terminated and remains alive for the duration of this synchronous Win32 call.
    unsafe { OutputDebugStringA(output.as_ptr()) };
}

#[cfg(not(target_os = "windows"))]
const fn write_windows_debug_line(_formatted: &[u8]) {}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use std::fs;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_log_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("smu-script-log-{name}-{}-{nonce}.log", std::process::id()))
    }

    fn payload_from_formatted_line(line: &[u8]) -> &[u8] {
        assert_eq!(line.first(), Some(&b'['));
        assert_eq!(line.get(5), Some(&b'-'));
        assert_eq!(line.get(8), Some(&b'-'));
        assert_eq!(line.get(11), Some(&b' '));
        assert_eq!(line.get(14), Some(&b':'));
        assert_eq!(line.get(17), Some(&b':'));
        assert_eq!(line.get(20..29), Some(b"] [INFO] ".as_slice()));
        &line[29..]
    }

    #[test]
    fn bounded_history_keeps_newest_cpp_sized_window() {
        let logger = AppScriptLogger::for_test(false, false, PathBuf::new());
        for index in 0..(MAX_LOG_ENTRIES + 3) {
            logger.log_info(format!("entry-{index}").as_bytes());
        }
        let entries = logger.snapshot();
        assert_eq!(entries.len(), MAX_LOG_ENTRIES);
        assert_eq!(entries.first().map(Vec::as_slice), Some(b"entry-3".as_slice()));
        assert_eq!(
            entries.last().map(Vec::as_slice),
            Some(format!("entry-{}", MAX_LOG_ENTRIES + 2).as_bytes())
        );
    }

    #[test]
    fn debug_file_preserves_cpp_timestamp_level_prefix_and_script_bytes() {
        let path = temp_log_path("bytes");
        let logger = AppScriptLogger::for_test(false, true, path.clone());
        logger.log_info(b"[script] hello\0world");
        let file = fs::read(&path).expect("debug log should be readable");
        let line = file.strip_suffix(b"\n").expect("debug log should end with newline");
        assert_eq!(payload_from_formatted_line(line), b"[script] hello\0world");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn warnings_use_cpp_level_format_and_drain_once() {
        let path = temp_log_path("warning");
        let logger = AppScriptLogger::for_test(false, true, path.clone());
        logger.log_warning(b"Imported script failed during onSettings: boom");

        let file = fs::read(&path).expect("warning log should be readable");
        let line = file.strip_suffix(b"\n").expect("warning log should end with newline");
        assert_eq!(line.get(20..29), Some(b"] [WARN] ".as_slice()));
        assert_eq!(&line[29..], b"Imported script failed during onSettings: boom");
        assert_eq!(
            logger.drain_warnings(),
            vec!["Imported script failed during onSettings: boom".to_owned()]
        );
        assert!(logger.drain_warnings().is_empty());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn warning_queue_keeps_newest_cpp_sized_window() {
        let logger = AppScriptLogger::for_test(false, false, PathBuf::new());
        for index in 0..(MAX_LOG_ENTRIES + 3) {
            logger.log_warning(format!("warning-{index}").as_bytes());
        }
        let warnings = logger.drain_warnings();
        assert_eq!(warnings.len(), MAX_LOG_ENTRIES);
        assert_eq!(warnings.first().map(String::as_str), Some("warning-3"));
        let expected_last = format!("warning-{}", MAX_LOG_ENTRIES + 2);
        assert_eq!(warnings.last().map(String::as_str), Some(expected_last.as_str()));
    }

    #[test]
    fn concurrent_file_logging_preserves_ring_order() {
        let path = temp_log_path("concurrent");
        let logger = Arc::new(AppScriptLogger::for_test(false, true, path.clone()));
        let mut workers = Vec::new();
        for worker in 0..8 {
            let logger = Arc::clone(&logger);
            workers.push(std::thread::spawn(move || {
                for index in 0..32 {
                    logger.log_info(format!("worker-{worker}-entry-{index}").as_bytes());
                }
            }));
        }
        for worker in workers {
            worker.join().expect("logging worker should finish");
        }

        let ring = logger.snapshot();
        let file = fs::read(&path).expect("debug log should be readable");
        let file_messages = file
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| payload_from_formatted_line(line).to_vec())
            .collect::<Vec<_>>();
        assert_eq!(file_messages, ring);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn default_debug_file_path_is_beside_current_executable() {
        let expected = std::env::current_exe()
            .expect("test executable path should be available")
            .parent()
            .expect("test executable should have a parent")
            .join(DEFAULT_LOG_FILENAME);
        assert_eq!(default_log_path(), expected);
    }
}
