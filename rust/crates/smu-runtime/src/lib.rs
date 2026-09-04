//! Timing and runtime primitives for compatibility-first macro execution.
//!
//! This crate deliberately does not choose the final threading model yet. It provides deterministic
//! time contracts that both controller-style and worker-style execution can consume in tests.

pub mod anti_afk;
pub mod lag_switch;
pub mod macros;
pub mod process_control;
pub mod trace;

use std::time::Duration;

/// Monotonic time in microseconds from an arbitrary process-local epoch.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct MonotonicMicros(u64);

impl MonotonicMicros {
    #[must_use]
    pub const fn from_micros(micros: u64) -> Self {
        Self(micros)
    }

    #[must_use]
    pub const fn as_micros(self) -> u64 {
        self.0
    }

    #[must_use]
    pub fn saturating_add(self, duration: Duration) -> Self {
        let micros = u64::try_from(duration.as_micros()).unwrap_or(u64::MAX);
        Self(self.0.saturating_add(micros))
    }
}

/// The semantic reason for a requested wait.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitKind {
    /// Ordinary coarse sleep, normally implemented with an efficient blocking primitive.
    Coarse,
    /// High-precision wait whose implementation may use a short bounded final spin/yield phase.
    Precise,
    /// Poll cadence used by the controller or another compatibility loop.
    Poll,
}

/// One wait request captured by deterministic tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WaitRecord {
    pub started_at: MonotonicMicros,
    pub duration: Duration,
    pub kind: WaitKind,
}

/// Minimal monotonic clock contract used by runtime logic.
pub trait Clock {
    fn now(&self) -> MonotonicMicros;
}

/// Minimal sleeper contract. Production implementations may block the current worker; fake
/// implementations advance virtual time immediately.
pub trait Sleeper: Clock {
    fn sleep(&mut self, duration: Duration, kind: WaitKind);
}

/// Deterministic clock/sleeper for compatibility trace tests.
#[derive(Debug, Default)]
pub struct FakeClock {
    now: MonotonicMicros,
    waits: Vec<WaitRecord>,
}

impl FakeClock {
    #[must_use]
    pub const fn at(now: MonotonicMicros) -> Self {
        Self { now, waits: Vec::new() }
    }

    #[must_use]
    pub fn waits(&self) -> &[WaitRecord] {
        &self.waits
    }

    pub fn advance(&mut self, duration: Duration) {
        self.now = self.now.saturating_add(duration);
    }

    pub fn clear_waits(&mut self) {
        self.waits.clear();
    }
}

impl Clock for FakeClock {
    fn now(&self) -> MonotonicMicros {
        self.now
    }
}

impl Sleeper for FakeClock {
    fn sleep(&mut self, duration: Duration, kind: WaitKind) {
        self.waits.push(WaitRecord { started_at: self.now, duration, kind });
        self.advance(duration);
    }
}

/// Oracle value used by the current C++ Lua precise-wait implementation.
pub const CPP_LUA_PRECISE_SPIN_TAIL: Duration = Duration::from_micros(600);

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{CPP_LUA_PRECISE_SPIN_TAIL, Clock, FakeClock, MonotonicMicros, Sleeper, WaitKind};

    #[test]
    fn fake_sleep_advances_and_records_exactly() {
        let mut clock = FakeClock::at(MonotonicMicros::from_micros(100));
        clock.sleep(Duration::from_micros(250), WaitKind::Precise);
        assert_eq!(clock.now().as_micros(), 350);
        assert_eq!(clock.waits().len(), 1);
        assert_eq!(clock.waits()[0].started_at.as_micros(), 100);
        assert_eq!(clock.waits()[0].duration, Duration::from_micros(250));
        assert_eq!(clock.waits()[0].kind, WaitKind::Precise);
    }

    #[test]
    fn fake_clock_saturates_instead_of_wrapping() {
        let mut clock = FakeClock::at(MonotonicMicros::from_micros(u64::MAX - 5));
        clock.advance(Duration::from_micros(10));
        assert_eq!(clock.now().as_micros(), u64::MAX);
    }

    #[test]
    fn cpp_precise_tail_is_frozen_as_compatibility_data() {
        assert_eq!(CPP_LUA_PRECISE_SPIN_TAIL, Duration::from_micros(600));
    }
}
