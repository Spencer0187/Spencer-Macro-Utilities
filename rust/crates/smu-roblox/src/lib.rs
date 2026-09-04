//! Roblox-specific services used by SMU.

pub mod log;

use std::sync::{Mutex, MutexGuard};

pub use log::{RobloxLogReader, RobloxLogServerInfo, RobloxLogSnapshot};

/// Thread-safe log snapshot capability consumed by runtime/scripting layers.
pub trait RobloxLogSource: Send + Sync {
    fn read_log(&self, include_existing: bool) -> RobloxLogSnapshot;
}

/// Shared wrapper around the incremental compatibility reader.
#[derive(Debug)]
pub struct SharedRobloxLogReader {
    reader: Mutex<RobloxLogReader>,
}

impl SharedRobloxLogReader {
    #[must_use]
    pub const fn new(reader: RobloxLogReader) -> Self {
        Self { reader: Mutex::new(reader) }
    }

    fn reader(&self) -> MutexGuard<'_, RobloxLogReader> {
        match self.reader.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

impl Default for SharedRobloxLogReader {
    fn default() -> Self {
        Self::new(RobloxLogReader::default())
    }
}

impl RobloxLogSource for SharedRobloxLogReader {
    fn read_log(&self, include_existing: bool) -> RobloxLogSnapshot {
        self.reader().poll(include_existing)
    }
}
