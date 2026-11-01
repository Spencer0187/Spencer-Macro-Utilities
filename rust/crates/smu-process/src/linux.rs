//! Linux `/proc` implementation of process discovery.

use crate::{ProcessDiscovery, ProcessRecord};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// All macro controllers share one process table. The C++ controller refreshes it once per
// second; independently scanning /proc from each Rust controller produces millions of syscalls.
const PROCESS_SNAPSHOT_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Default)]
struct SnapshotCache {
    checked_at: Option<Instant>,
    // A cheap, immutable shared view is used by the high-frequency foreground check.
    // Generic ProcessDiscovery consumers retain the original owned-Vec API.
    records: Arc<Vec<ProcessRecord>>,
}

mod control;
mod foreground;
pub use control::{
    FreezeHelperAuthorization, FreezeHelperClient, ProcCgroupProcessControl, UnixFreezeHelper,
};
pub use foreground::{
    DisplayServer, DisplayServerSource, EnvironmentDisplayServer, ForegroundPidSource,
    ProcForegroundProcess, X11rbForegroundSource, detect_display_server,
    foreground_detection_available,
};

/// Linux process discovery backed by a procfs tree.
#[derive(Clone, Debug)]
pub struct ProcDiscovery {
    proc_root: PathBuf,
    cache: Arc<Mutex<SnapshotCache>>,
}

impl Default for ProcDiscovery {
    fn default() -> Self {
        Self::new("/proc")
    }
}

impl ProcDiscovery {
    #[must_use]
    pub fn new(proc_root: impl Into<PathBuf>) -> Self {
        Self { proc_root: proc_root.into(), cache: Arc::new(Mutex::new(SnapshotCache::default())) }
    }

    pub(super) fn snapshot_shared(&self) -> io::Result<Arc<Vec<ProcessRecord>>> {
        let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        if cache
            .checked_at
            .is_some_and(|checked| now.duration_since(checked) < PROCESS_SNAPSHOT_INTERVAL)
        {
            return Ok(Arc::clone(&cache.records));
        }
        let mut records = Vec::new();
        for entry in fs::read_dir(&self.proc_root)? {
            let Ok(entry) = entry else {
                continue;
            };
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(pid) = parse_proc_pid(&name) else {
                continue;
            };
            records.push(self.record_for_pid(pid));
        }
        records.sort_unstable_by_key(|record| record.pid);
        cache.checked_at = Some(Instant::now());
        cache.records = Arc::new(records);
        Ok(Arc::clone(&cache.records))
    }

    fn record_for_pid(&self, pid: u32) -> ProcessRecord {
        let process_root = self.proc_root.join(pid.to_string());
        // /proc/<pid>/stat carries the same TASK_COMM name as /proc/<pid>/comm, plus PPID.
        // Reuse one read for both instead of opening /proc/<pid>/comm for every process.
        let (stat_name, parent_pid) = read_stat_fields(&process_root.join("stat"));
        ProcessRecord {
            pid,
            parent_pid,
            command_name: stat_name.or_else(|| read_first_line(&process_root.join("comm"))),
            executable_name: read_executable_name(&process_root.join("exe")),
        }
    }
}

impl ProcessDiscovery for ProcDiscovery {
    fn snapshot(&self) -> io::Result<Vec<ProcessRecord>> {
        Ok((*self.snapshot_shared()?).clone())
    }
}

fn parse_proc_pid(name: &str) -> Option<u32> {
    if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let pid = name.parse::<u32>().ok()?;
    (pid != 0).then_some(pid)
}

fn read_first_line(path: &Path) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
    contents.lines().next().map(str::to_owned)
}

fn read_executable_name(path: &Path) -> Option<String> {
    let target = fs::read_link(path).ok()?;
    target.file_name()?.to_str().map(str::to_owned)
}

fn read_stat_fields(path: &Path) -> (Option<String>, Option<u32>) {
    let Ok(line) = fs::read_to_string(path) else {
        return (None, None);
    };
    let (Some(open), Some(close)) = (line.find('('), line.rfind(')')) else {
        return (None, None);
    };
    if close <= open {
        return (None, None);
    }
    // The comm field may itself contain spaces and parentheses; use the closing delimiter
    // instead of positional whitespace parsing, just as the old parent-PID parser did.
    let name = line.get(open + 1..close).map(str::to_owned);
    let parent_pid = line.get(close + 2..).and_then(|fields| {
        let mut fields = fields.split_ascii_whitespace();
        let _state = fields.next()?;
        fields.next()?.parse::<u32>().ok()
    });
    (name, parent_pid)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::{parse_proc_pid, read_stat_fields};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn proc_pid_parser_matches_nonzero_decimal_contract() {
        assert_eq!(parse_proc_pid("1"), Some(1));
        assert_eq!(parse_proc_pid("0012"), Some(12));
        assert_eq!(parse_proc_pid("0"), None);
        assert_eq!(parse_proc_pid("+12"), None);
        assert_eq!(parse_proc_pid("12x"), None);
    }

    #[test]
    fn proc_discovery_snapshot_drives_legacy_selection() {
        use crate::ProcessDiscovery;
        use std::os::unix::fs::symlink;

        let unique =
            SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_nanos());
        let root = std::env::temp_dir().join(format!("smu-process-proc-{unique}"));
        let launcher = root.join("20");
        let game = root.join("21");
        fs::create_dir_all(&launcher).expect("launcher proc fixture should be creatable");
        fs::create_dir_all(&game).expect("game proc fixture should be creatable");
        fs::write(launcher.join("comm"), "sober\n")
            .expect("launcher comm fixture should be writable");
        fs::write(launcher.join("stat"), "20 (sober) S 1 0 0 0\n")
            .expect("launcher stat fixture should be writable");
        symlink("/app/bin/sober", launcher.join("exe"))
            .expect("launcher exe fixture should be linkable");
        fs::write(game.join("comm"), "Main\n").expect("game comm fixture should be writable");
        fs::write(game.join("stat"), "21 (Main) S 20 0 0 0\n")
            .expect("game stat fixture should be writable");
        symlink("/app/bin/sober", game.join("exe")).expect("game exe fixture should be linkable");

        let discovery = super::ProcDiscovery::new(&root);
        assert_eq!(
            discovery.find_all_processes("sober").expect("fixture proc scan should succeed"),
            vec![20, 21]
        );
        assert_eq!(
            discovery.find_main_process("sober").expect("fixture proc scan should succeed"),
            Some(21)
        );

        fs::remove_dir_all(root).expect("temporary proc fixture should be removable");
    }

    #[test]
    fn cloned_discovery_shares_one_snapshot_until_refresh_due() {
        use crate::ProcessDiscovery;
        use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

        let unique =
            SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_nanos());
        let root = std::env::temp_dir().join(format!("smu-process-cache-{unique}"));
        let first = root.join("111");
        fs::create_dir_all(&first).expect("fixture should be creatable");
        fs::write(first.join("comm"), "Main\n").expect("comm should be writable");

        let discovery = super::ProcDiscovery::new(&root);
        let clone = discovery.clone();
        let initial_shared = discovery.snapshot_shared().expect("initial shared snapshot");
        assert!(std::sync::Arc::ptr_eq(
            &initial_shared,
            &clone.snapshot_shared().expect("cloned view should reuse allocation")
        ));
        assert_eq!(discovery.snapshot().expect("initial snapshot").len(), 1);
        let second = root.join("222");
        fs::create_dir_all(&second).expect("second process fixture should be creatable");
        fs::write(second.join("comm"), "Main\n").expect("second comm should be writable");
        assert_eq!(clone.snapshot().expect("cached snapshot").len(), 1);

        // Advance the cache clock without sleeping for a full second in the test.
        let mut cache = discovery.cache.lock().expect("cache lock");
        cache.checked_at =
            Some(Instant::now().checked_sub(Duration::from_secs(2)).expect("valid prior instant"));
        drop(cache);
        let refreshed_shared = clone.snapshot_shared().expect("refreshed shared snapshot");
        assert_eq!(refreshed_shared.len(), 2);
        assert!(!std::sync::Arc::ptr_eq(&initial_shared, &refreshed_shared));
        assert_eq!(clone.snapshot().expect("refreshed snapshot").len(), 2);
        fs::remove_dir_all(root).expect("fixture cleanup");
    }

    #[test]
    fn stat_parent_parser_handles_spaces_and_parens_in_process_name() {
        let unique =
            SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_nanos());
        let path = std::env::temp_dir().join(format!("smu-process-stat-{unique}"));
        fs::write(&path, "123 (odd name) with ) paren) S 456 0 0 0\n")
            .expect("temporary stat fixture should be writable");
        assert_eq!(read_stat_fields(&path), (Some("odd name) with ) paren".to_owned()), Some(456)));
        let _ = fs::remove_file(path);
    }
}
