//! Shared target-process selection and foreground-gating cache for live macro controllers.
//!
//! The C++ runtime refreshes process matches at most once per second and foreground eligibility at
//! most every 25 ms. Foreground matching always uses every matching target process, even when a
//! macro's process-control action intentionally targets only the main/newest process.

use smu_process::{ProcessDiscovery, ProcessForeground};
use std::time::{Duration, Instant};

const PROCESS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const FOREGROUND_REFRESH_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug)]
pub struct TargetProcessCache {
    selected_pids: Vec<u32>,
    foreground_pids: Vec<u32>,
    last_selector: Option<String>,
    last_take_all_process_ids: bool,
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
            last_selector: None,
            last_take_all_process_ids: false,
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
        if now < self.next_process_refresh
            && self.last_selector.as_deref() == Some(selector)
            && self.last_take_all_process_ids == take_all_process_ids
        {
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
        // Linux matches `PlatformCapabilities::canDetectForegroundProcess`: foreground gating
        // requires a usable X11/EWMH active-window path. Windows has a direct foreground-window PID
        // query, so it does not need this fallback.
        #[cfg(target_os = "linux")]
        if !smu_process::linux::foreground_detection_available() {
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
        if self.last_selector.as_deref() != Some(selector) {
            self.last_selector = Some(selector.to_owned());
        }
        self.last_take_all_process_ids = take_all_process_ids;
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
    #[cfg(target_os = "linux")]
    {
        // Keep the recent Linux optimization: both selections come from one cached procfs
        // generation. Two trait-default calls would deep-clone the table twice per controller.
        let Ok(records) = discovery.snapshot() else {
            return (Vec::new(), Vec::new());
        };
        let all = smu_process::select_all_processes(selector, &records);
        let selected = if take_all_process_ids {
            all.clone()
        } else {
            smu_process::select_main_process(selector, &records).into_iter().collect()
        };
        (selected, all)
    }
    #[cfg(not(target_os = "linux"))]
    {
        discover_native_targets(selector, take_all_process_ids, discovery)
    }
}

#[cfg(any(not(target_os = "linux"), test))]
fn discover_native_targets(
    selector: &str,
    take_all_process_ids: bool,
    discovery: &dyn ProcessDiscovery,
) -> (Vec<u32>, Vec<u32>) {
    // Windows uses case-insensitive names and newest creation time; macOS uses bundle aliases
    // and the frontmost process. Only the native backend overrides know those rules.
    // Windows and macOS cache enumeration, so these do not perform two independent OS scans.
    let all = discovery.find_all_processes(selector).unwrap_or_default();
    let selected = if take_all_process_ids {
        all.clone()
    } else {
        discovery.find_main_process(selector).ok().flatten().into_iter().collect()
    };
    (selected, all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use smu_process::ProcessRecord;
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    #[derive(Debug, Default)]
    struct NativeSelectionDiscovery {
        snapshot_scans: AtomicUsize,
        all_enumerations: AtomicUsize,
        main_lookups: AtomicUsize,
    }

    impl ProcessDiscovery for NativeSelectionDiscovery {
        fn snapshot(&self) -> std::io::Result<Vec<ProcessRecord>> {
            self.snapshot_scans.fetch_add(1, Ordering::Relaxed);
            Ok(vec![])
        }

        fn find_all_processes(&self, selector: &str) -> std::io::Result<Vec<u32>> {
            self.all_enumerations.fetch_add(1, Ordering::Relaxed);
            Ok(if selector == "other" { vec![41, 42] } else { vec![10, 11] })
        }

        fn find_main_process(&self, selector: &str) -> std::io::Result<Option<u32>> {
            self.main_lookups.fetch_add(1, Ordering::Relaxed);
            Ok(Some(if selector == "other" { 42 } else { 11 }))
        }
    }

    #[test]
    fn controller_uses_platform_selection_overrides_not_linux_snapshot_semantics() {
        let backend = NativeSelectionDiscovery::default();
        assert_eq!(discover_native_targets("game", false, &backend), (vec![11], vec![10, 11]));
        assert_eq!(discover_native_targets("other", true, &backend), (vec![41, 42], vec![41, 42]));
        assert_eq!(backend.all_enumerations.load(Ordering::Relaxed), 2);
        assert_eq!(backend.main_lookups.load(Ordering::Relaxed), 1);
        assert_eq!(backend.snapshot_scans.load(Ordering::Relaxed), 0);
    }

    #[cfg(target_os = "linux")]
    #[derive(Debug, Default)]
    struct CountingLinuxDiscovery {
        snapshots: AtomicUsize,
    }

    #[cfg(target_os = "linux")]
    impl ProcessDiscovery for CountingLinuxDiscovery {
        fn snapshot(&self) -> std::io::Result<Vec<ProcessRecord>> {
            self.snapshots.fetch_add(1, Ordering::Relaxed);
            Ok(records())
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn target_cache_refreshes_on_selector_or_mode_change_without_repeating_scans() {
        let backend = CountingLinuxDiscovery::default();
        let mut cache = TargetProcessCache::default();
        cache.refresh_if_due("game", false, &backend);
        assert_eq!(cache.selected_pids(), &[10]);
        cache.refresh_if_due("game", false, &backend);
        assert_eq!(backend.snapshots.load(Ordering::Relaxed), 1);

        cache.refresh_if_due("game", true, &backend);
        assert_eq!(cache.selected_pids(), &[10, 20]);
        cache.refresh_if_due("other", false, &backend);
        assert!(cache.selected_pids().is_empty());
        assert_eq!(backend.snapshots.load(Ordering::Relaxed), 3);
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn native_target_cache_refreshes_on_selector_or_mode_change() {
        let backend = NativeSelectionDiscovery::default();
        let mut cache = TargetProcessCache::default();
        cache.refresh_if_due("game", false, &backend);
        assert_eq!(cache.selected_pids(), &[11]);
        cache.refresh_if_due("game", false, &backend);
        assert_eq!(backend.all_enumerations.load(Ordering::Relaxed), 1);
        cache.refresh_if_due("other", true, &backend);
        assert_eq!(cache.selected_pids(), &[41, 42]);
        assert_eq!(backend.all_enumerations.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn process_control_selection_uses_main_or_all_but_foreground_always_keeps_all() {
        let discovery = FakeDiscovery(records());
        assert_eq!(discover_targets("game", false, &discovery), (vec![10], vec![10, 20]));
        assert_eq!(discover_targets("game", true, &discovery), (vec![10, 20], vec![10, 20]));
    }
}
