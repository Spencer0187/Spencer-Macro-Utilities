//! Shared target-process selection and foreground-gating cache for live macro controllers.
//!
//! The C++ runtime refreshes process matches at most once per second and foreground eligibility at
//! most every 25 ms. Foreground matching always uses every matching target process, even when a
//! macro's process-control action intentionally targets only the main/newest process.

use smu_process::linux::{DisplayServer, detect_display_server};
use smu_process::{ProcessDiscovery, ProcessForeground, select_all_processes, select_main_process};
use std::time::{Duration, Instant};

const PROCESS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const FOREGROUND_REFRESH_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug)]
pub struct TargetProcessCache {
    selected_pids: Vec<u32>,
    foreground_pids: Vec<u32>,
    next_process_refresh: Instant,
    next_foreground_check: Instant,
    cached_foreground_allowed: bool,
}

impl Default for TargetProcessCache {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            selected_pids: Vec::new(),
            foreground_pids: Vec::new(),
            next_process_refresh: now,
            next_foreground_check: now,
            cached_foreground_allowed: true,
        }
    }
}

impl TargetProcessCache {
    pub fn refresh_if_due(
        &mut self,
        selector: &str,
        take_all_process_ids: bool,
        discovery: &dyn ProcessDiscovery,
    ) {
        let now = Instant::now();
        if now < self.next_process_refresh {
            return;
        }
        self.next_process_refresh = now + PROCESS_REFRESH_INTERVAL;
        self.refresh_now_at(selector, take_all_process_ids, discovery, now);
    }

    pub fn force_refresh(
        &mut self,
        selector: &str,
        take_all_process_ids: bool,
        discovery: &dyn ProcessDiscovery,
    ) {
        let now = Instant::now();
        self.next_process_refresh = now + PROCESS_REFRESH_INTERVAL;
        self.refresh_now_at(selector, take_all_process_ids, discovery, now);
    }

    #[must_use]
    pub fn selected_pids(&self) -> &[u32] {
        &self.selected_pids
    }

    #[must_use]
    pub fn foreground_allowed(
        &mut self,
        disable_outside_target: bool,
        foreground: &dyn ProcessForeground,
    ) -> bool {
        if !disable_outside_target {
            return true;
        }
        // Matches `PlatformCapabilities::canDetectForegroundProcess`: native Wayland and unknown
        // sessions deliberately allow macros rather than pretending a failed X11 lookup is a miss.
        if detect_display_server() != DisplayServer::X11 {
            return true;
        }

        let now = Instant::now();
        if now < self.next_foreground_check {
            return self.cached_foreground_allowed;
        }
        self.next_foreground_check = now + FOREGROUND_REFRESH_INTERVAL;
        self.cached_foreground_allowed =
            self.foreground_pids.iter().copied().any(|pid| foreground.is_foreground_process(pid));
        self.cached_foreground_allowed
    }

    fn refresh_now_at(
        &mut self,
        selector: &str,
        take_all_process_ids: bool,
        discovery: &dyn ProcessDiscovery,
        now: Instant,
    ) {
        let (selected, foreground) = discover_targets(selector, take_all_process_ids, discovery);
        if selected != self.selected_pids || foreground != self.foreground_pids {
            self.next_foreground_check = now;
        }
        self.selected_pids = selected;
        self.foreground_pids = foreground;
    }
}

#[must_use]
pub fn discover_targets(
    selector: &str,
    take_all_process_ids: bool,
    discovery: &dyn ProcessDiscovery,
) -> (Vec<u32>, Vec<u32>) {
    let Ok(records) = discovery.snapshot() else {
        return (Vec::new(), Vec::new());
    };
    let all = select_all_processes(selector, &records);
    let selected = if take_all_process_ids {
        all.clone()
    } else {
        select_main_process(selector, &records).into_iter().collect()
    };
    (selected, all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use smu_process::ProcessRecord;

    #[derive(Debug)]
    struct FakeDiscovery(Vec<ProcessRecord>);

    impl ProcessDiscovery for FakeDiscovery {
        fn snapshot(&self) -> std::io::Result<Vec<ProcessRecord>> {
            Ok(self.0.clone())
        }
    }

    fn records() -> Vec<ProcessRecord> {
        vec![
            ProcessRecord {
                pid: 10,
                parent_pid: Some(1),
                command_name: Some("game".to_owned()),
                executable_name: Some("game".to_owned()),
            },
            ProcessRecord {
                pid: 20,
                parent_pid: Some(10),
                command_name: Some("game".to_owned()),
                executable_name: Some("game".to_owned()),
            },
        ]
    }

    #[test]
    fn process_control_selection_uses_main_or_all_but_foreground_always_keeps_all() {
        let discovery = FakeDiscovery(records());
        assert_eq!(discover_targets("game", false, &discovery), (vec![10], vec![10, 20]));
        assert_eq!(discover_targets("game", true, &discovery), (vec![10, 20], vec![10, 20]));
    }
}
