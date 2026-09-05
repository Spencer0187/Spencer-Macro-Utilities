//! Linux `/proc` implementation of process discovery.

use crate::{ProcessDiscovery, ProcessRecord};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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
}

impl Default for ProcDiscovery {
    fn default() -> Self {
        Self::new("/proc")
    }
}

impl ProcDiscovery {
    #[must_use]
    pub fn new(proc_root: impl Into<PathBuf>) -> Self {
        Self { proc_root: proc_root.into() }
    }

    fn record_for_pid(&self, pid: u32) -> ProcessRecord {
        let process_root = self.proc_root.join(pid.to_string());
        ProcessRecord {
            pid,
            parent_pid: read_parent_pid(&process_root.join("stat")),
            command_name: read_first_line(&process_root.join("comm")),
            executable_name: read_executable_name(&process_root.join("exe")),
        }
    }
}

impl ProcessDiscovery for ProcDiscovery {
    fn snapshot(&self) -> io::Result<Vec<ProcessRecord>> {
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
        Ok(records)
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

fn read_parent_pid(path: &Path) -> Option<u32> {
    let line = fs::read_to_string(path).ok()?;
    let end_name = line.rfind(')')?;
    let fields = line.get(end_name.saturating_add(2)..)?;
    let mut fields = fields.split_ascii_whitespace();
    let _state = fields.next()?;
    fields.next()?.parse::<u32>().ok()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::{parse_proc_pid, read_parent_pid};
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
    fn stat_parent_parser_handles_spaces_and_parens_in_process_name() {
        let unique =
            SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_nanos());
        let path = std::env::temp_dir().join(format!("smu-process-stat-{unique}"));
        fs::write(&path, "123 (odd name) with ) paren) S 456 0 0 0\n")
            .expect("temporary stat fixture should be writable");
        assert_eq!(read_parent_pid(&path), Some(456));
        let _ = fs::remove_file(path);
    }
}
