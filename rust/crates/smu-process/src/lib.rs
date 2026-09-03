//! Process discovery and selection services for the Rust rewrite.
//!
//! This crate starts with the compatibility-sensitive discovery half of the old `ProcessBackend`.
//! Process suspension and foreground-window integration remain separate capabilities so they can be
//! implemented without recreating the C++ platform-backend grab bag.

use std::collections::BTreeSet;
use std::io;

/// One process row used by selector compatibility logic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessRecord {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub command_name: Option<String>,
    pub executable_name: Option<String>,
}

impl ProcessRecord {
    #[must_use]
    pub const fn new(pid: u32) -> Self {
        Self { pid, parent_pid: None, command_name: None, executable_name: None }
    }
}

/// Source of process snapshots.
pub trait ProcessDiscovery: Send + Sync {
    /// Capture the currently visible process table.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the platform process table itself cannot be enumerated.
    fn snapshot(&self) -> io::Result<Vec<ProcessRecord>>;

    /// Find the first process matching the legacy SMU selector.
    ///
    /// # Errors
    ///
    /// Propagates process-table enumeration failures.
    fn find_process(&self, selector: &str) -> io::Result<Option<u32>> {
        self.find_all_processes(selector).map(|pids| pids.into_iter().next())
    }

    /// Find all processes matching the legacy SMU selector.
    ///
    /// # Errors
    ///
    /// Propagates process-table enumeration failures.
    fn find_all_processes(&self, selector: &str) -> io::Result<Vec<u32>> {
        self.snapshot().map(|records| select_all_processes(selector, &records))
    }

    /// Find the process SMU should treat as the main process.
    ///
    /// # Errors
    ///
    /// Propagates process-table enumeration failures.
    fn find_main_process(&self, selector: &str) -> io::Result<Option<u32>> {
        self.snapshot().map(|records| select_main_process(selector, &records))
    }
}

fn parse_pid_token(token: &str) -> Option<u32> {
    if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let parsed = token.parse::<u32>().ok()?;
    (parsed != 0).then_some(parsed)
}

const fn is_sober_alias(value: &str) -> bool {
    value.eq_ignore_ascii_case("sober")
}

const fn is_main_alias(value: &str) -> bool {
    value.eq_ignore_ascii_case("main")
}

fn process_matches_executable(record: &ProcessRecord, executable: &str) -> bool {
    if record.command_name.as_deref() == Some(executable)
        || record.executable_name.as_deref() == Some(executable)
    {
        return true;
    }

    if is_sober_alias(executable) {
        let command_is_sober =
            record.command_name.as_deref().is_some_and(|value| value.eq_ignore_ascii_case("sober"));
        let executable_is_sober = record
            .executable_name
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("sober"));
        if command_is_sober || executable_is_sober {
            return true;
        }

        let command_is_main =
            record.command_name.as_deref().is_some_and(|value| value.eq_ignore_ascii_case("main"));
        if command_is_main && executable_is_sober {
            return true;
        }
    }

    if is_main_alias(executable) {
        let command_is_main =
            record.command_name.as_deref().is_some_and(|value| value.eq_ignore_ascii_case("main"));
        let executable_is_sober_or_unknown = record
            .executable_name
            .as_deref()
            .is_none_or(|value| value.eq_ignore_ascii_case("sober"));
        if command_is_main && executable_is_sober_or_unknown {
            return true;
        }
    }

    false
}

/// Apply the exact Linux selector rules currently used by the C++ process backend.
#[must_use]
pub fn select_all_processes(selector: &str, records: &[ProcessRecord]) -> Vec<u32> {
    let existing_pids: BTreeSet<u32> = records.iter().map(|record| record.pid).collect();
    let mut pids = Vec::new();
    let mut executable_tokens = Vec::new();

    for token in selector.split_ascii_whitespace() {
        if let Some(pid) = parse_pid_token(token) {
            if existing_pids.contains(&pid) {
                pids.push(pid);
            }
        } else {
            executable_tokens.push(token);
        }
    }

    if !executable_tokens.is_empty() {
        for record in records {
            if executable_tokens
                .iter()
                .any(|executable| process_matches_executable(record, executable))
            {
                pids.push(record.pid);
            }
        }
    }

    pids.sort_unstable();
    pids.dedup();
    pids
}

/// Apply the current C++ main-process selection heuristic to a captured process table.
#[must_use]
pub fn select_main_process(selector: &str, records: &[ProcessRecord]) -> Option<u32> {
    let pids = select_all_processes(selector, records);
    if pids.is_empty() {
        return None;
    }

    if is_sober_alias(selector) || is_main_alias(selector) {
        for pid in &pids {
            let Some(record) = records.iter().find(|record| record.pid == *pid) else {
                continue;
            };
            let command_is_main = record
                .command_name
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case("main"));
            let executable_is_sober = record
                .executable_name
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case("sober"));
            if command_is_main && executable_is_sober {
                return Some(*pid);
            }
        }
    }

    if pids.len() == 1 {
        return pids.first().copied();
    }

    let pid_set: BTreeSet<u32> = pids.iter().copied().collect();
    for pid in &pids {
        let parent =
            records.iter().find(|record| record.pid == *pid).and_then(|record| record.parent_pid);
        if parent.is_none_or(|parent_pid| !pid_set.contains(&parent_pid)) {
            return Some(*pid);
        }
    }

    pids.first().copied()
}

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(test)]
mod tests {
    use super::{ProcessRecord, select_all_processes, select_main_process};

    fn record(
        pid: u32,
        parent_pid: Option<u32>,
        command_name: Option<&str>,
        executable_name: Option<&str>,
    ) -> ProcessRecord {
        ProcessRecord {
            pid,
            parent_pid,
            command_name: command_name.map(str::to_owned),
            executable_name: executable_name.map(str::to_owned),
        }
    }

    #[test]
    fn numeric_and_named_selectors_sort_and_deduplicate_like_cpp() {
        let records = [
            record(42, Some(1), Some("RobloxPlayerBeta.exe"), None),
            record(7, Some(1), Some("other"), None),
            record(99, Some(1), None, Some("RobloxPlayerBeta.exe")),
        ];

        assert_eq!(
            select_all_processes("99 RobloxPlayerBeta.exe 42 99 0 +7", &records),
            vec![42, 99]
        );
    }

    #[test]
    fn sober_and_main_aliases_preserve_cpp_matching_rules() {
        let records = [
            record(10, Some(1), Some("sober"), Some("sober")),
            record(11, Some(10), Some("Main"), Some("sober")),
            record(12, Some(1), Some("MAIN"), None),
            record(13, Some(1), Some("Main"), Some("unrelated")),
        ];

        assert_eq!(select_all_processes("SoBeR", &records), vec![10, 11]);
        assert_eq!(select_all_processes("main", &records), vec![11, 12]);
    }

    #[test]
    fn exact_name_matching_remains_case_sensitive_outside_aliases() {
        let records = [
            record(2, Some(1), Some("RobloxPlayerBeta.exe"), None),
            record(3, Some(1), Some("robloxplayerbeta.exe"), None),
        ];

        assert_eq!(select_all_processes("RobloxPlayerBeta.exe", &records), vec![2]);
    }

    #[test]
    fn sober_main_process_is_preferred_over_launcher_tree_root() {
        let records = [
            record(20, Some(1), Some("sober"), Some("sober")),
            record(21, Some(20), Some("Main"), Some("sober")),
            record(22, Some(20), Some("sober"), Some("sober")),
        ];

        assert_eq!(select_main_process("sober", &records), Some(21));
        assert_eq!(select_main_process("MAIN", &records), Some(21));
    }

    #[test]
    fn generic_main_process_uses_first_sorted_tree_root() {
        let records = [
            record(40, Some(30), Some("game"), None),
            record(30, Some(1), Some("game"), None),
            record(50, Some(2), Some("game"), None),
        ];

        assert_eq!(select_main_process("game", &records), Some(30));
    }

    #[test]
    fn cyclic_parent_data_falls_back_to_lowest_pid() {
        let records =
            [record(8, Some(9), Some("game"), None), record(9, Some(8), Some("game"), None)];

        assert_eq!(select_main_process("game", &records), Some(8));
    }

    #[test]
    fn multi_token_selector_does_not_trigger_sober_special_main_override() {
        let records = [
            record(20, Some(1), Some("sober"), Some("sober")),
            record(21, Some(20), Some("Main"), Some("sober")),
        ];

        assert_eq!(select_main_process("sober 20", &records), Some(20));
    }
}
