use smu_process::ProcessDiscovery;
use std::{
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

#[derive(Debug, Default)]
struct ProcessStatusCache {
    selector: String,
    found: bool,
    last_checked: Option<Instant>,
}

pub struct ProcessStatusService {
    discovery: Option<Arc<dyn ProcessDiscovery>>,
    cache: Mutex<ProcessStatusCache>,
    parity_override: Option<bool>,
}

impl ProcessStatusService {
    #[must_use]
    pub fn new(
        discovery: Option<Arc<dyn ProcessDiscovery>>,
        parity_override: Option<bool>,
    ) -> Self {
        Self { discovery, cache: Mutex::new(ProcessStatusCache::default()), parity_override }
    }

    #[must_use]
    pub fn found(&self, selector: &str) -> bool {
        if let Some(found) = self.parity_override {
            return found;
        }
        let Some(discovery) = self.discovery.as_ref() else {
            return false;
        };
        let mut cache = lock(&self.cache);
        let due = cache.selector != selector
            || cache.last_checked.is_none_or(|checked| checked.elapsed() >= Duration::from_secs(1));
        if due {
            selector.clone_into(&mut cache.selector);
            cache.found = discovery.find_main_process(selector).ok().flatten().is_some();
            cache.last_checked = Some(Instant::now());
        }
        cache.found
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::ProcessStatusService;
    use smu_process::{ProcessDiscovery, ProcessRecord};
    use std::{
        io,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    #[derive(Debug)]
    struct CountingDiscovery {
        calls: AtomicUsize,
    }

    impl CountingDiscovery {
        fn new() -> Self {
            Self { calls: AtomicUsize::new(0) }
        }
    }

    impl ProcessDiscovery for CountingDiscovery {
        fn snapshot(&self) -> io::Result<Vec<ProcessRecord>> {
            Ok(Vec::new())
        }

        fn find_main_process(&self, selector: &str) -> io::Result<Option<u32>> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok((selector == "found").then_some(42))
        }
    }

    #[test]
    fn same_selector_uses_cached_result_and_selector_change_refreshes() {
        let discovery = Arc::new(CountingDiscovery::new());
        let backend: Arc<dyn ProcessDiscovery> = discovery.clone();
        let service = ProcessStatusService::new(Some(backend), None);

        assert!(service.found("found"));
        assert!(service.found("found"));
        assert!(!service.found("missing"));
        assert_eq!(discovery.calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn parity_override_skips_backend_and_missing_backend_is_false() {
        let discovery = Arc::new(CountingDiscovery::new());
        let backend: Arc<dyn ProcessDiscovery> = discovery.clone();
        let forced = ProcessStatusService::new(Some(backend), Some(true));
        assert!(forced.found("anything"));
        assert_eq!(discovery.calls.load(Ordering::Relaxed), 0);

        let missing = ProcessStatusService::new(None, None);
        assert!(!missing.found("anything"));
    }
}
