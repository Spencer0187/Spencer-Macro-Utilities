//! Non-blocking, bounded input-backend diagnostics for the GUI's existing warning logger.
//! Input injectors cannot depend on the application crate; the UI drains these messages.
use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

const MAX_PENDING_WARNINGS: usize = 64;
const DUPLICATE_WARNING_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Default)]
struct InputWarnings {
    pending: VecDeque<String>,
    last_reports: VecDeque<(String, Instant)>,
}

impl InputWarnings {
    fn report_at(&mut self, message: String, now: Instant) {
        if self.last_reports.iter().rev().any(|(previous, last)| {
            previous == &message
                && now.saturating_duration_since(*last) < DUPLICATE_WARNING_INTERVAL
        }) {
            return;
        }
        if self.last_reports.len() == MAX_PENDING_WARNINGS {
            self.last_reports.pop_front();
        }
        self.last_reports.push_back((message.clone(), now));
        if self.pending.len() == MAX_PENDING_WARNINGS {
            self.pending.pop_front();
        }
        self.pending.push_back(message);
    }
}

static INPUT_WARNINGS: OnceLock<Mutex<InputWarnings>> = OnceLock::new();

/// Report a failed OS input injection. Called only on failures, never on the hot success path.
pub fn report_failure(message: impl Into<String>) {
    let warnings = INPUT_WARNINGS.get_or_init(|| Mutex::new(InputWarnings::default()));
    let mut warnings = warnings.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    warnings.report_at(message.into(), Instant::now());
}

/// Transfer failures to SMU's user-visible warning toast and optional persistent debug log.
#[must_use]
pub fn drain_warnings() -> Vec<String> {
    let Some(warnings) = INPUT_WARNINGS.get() else {
        return Vec::new();
    };
    let mut warnings = warnings.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    warnings.pending.drain(..).collect()
}

#[cfg(test)]
mod tests {
    use super::{DUPLICATE_WARNING_INTERVAL, InputWarnings, MAX_PENDING_WARNINGS};
    use std::time::{Duration, Instant};

    #[test]
    fn duplicate_failures_are_rate_limited_and_distinct_errors_are_visible() {
        let mut warnings = InputWarnings::default();
        let now = Instant::now();
        warnings.report_at("failed injection".into(), now);
        warnings.report_at("failed injection".into(), now + Duration::from_secs(1));
        warnings.report_at("device disconnected".into(), now + Duration::from_secs(2));
        warnings.report_at("failed injection".into(), now + Duration::from_secs(3));
        warnings.report_at("failed injection".into(), now + DUPLICATE_WARNING_INTERVAL);
        warnings.report_at(
            "device disconnected".into(),
            now + DUPLICATE_WARNING_INTERVAL + Duration::from_secs(2),
        );
        assert_eq!(warnings.pending.len(), 4);
        assert_eq!(warnings.pending.pop_front().as_deref(), Some("failed injection"));
        assert_eq!(warnings.pending.pop_front().as_deref(), Some("device disconnected"));
    }

    #[test]
    fn warning_queue_is_bounded_when_multiple_distinct_failures_arrive() {
        let mut warnings = InputWarnings::default();
        let now = Instant::now();
        for n in 0..MAX_PENDING_WARNINGS + 3 {
            warnings.report_at(format!("failure-{n}"), now);
        }
        assert_eq!(warnings.pending.len(), MAX_PENDING_WARNINGS);
        assert_eq!(warnings.pending.front().map(String::as_str), Some("failure-3"));
    }
}
