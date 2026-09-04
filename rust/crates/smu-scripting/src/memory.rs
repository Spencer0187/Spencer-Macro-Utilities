//! Lua memory-limit bookkeeping isolated from the VM allocator callback.

pub const DEFAULT_SCRIPT_MEMORY_LIMIT_MB: usize = 64;
pub const MIN_SCRIPT_MEMORY_LIMIT_MB: usize = 16;
pub const MAX_SCRIPT_MEMORY_LIMIT_MB: usize = 256;

#[must_use]
pub fn memory_limit_bytes(requested_mb: Option<usize>) -> usize {
    requested_mb
        .unwrap_or(DEFAULT_SCRIPT_MEMORY_LIMIT_MB)
        .clamp(MIN_SCRIPT_MEMORY_LIMIT_MB, MAX_SCRIPT_MEMORY_LIMIT_MB)
        .saturating_mul(1024)
        .saturating_mul(1024)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LuaMemoryBudget {
    limit_bytes: usize,
    used_bytes: usize,
}

impl LuaMemoryBudget {
    #[must_use]
    pub fn new(requested_mb: Option<usize>) -> Self {
        Self { limit_bytes: memory_limit_bytes(requested_mb), used_bytes: 0 }
    }

    #[must_use]
    pub const fn limit_bytes(self) -> usize {
        self.limit_bytes
    }

    #[must_use]
    pub const fn used_bytes(self) -> usize {
        self.used_bytes
    }

    /// Test the same growth condition as the C++ allocator before `malloc`/`realloc` is attempted.
    #[must_use]
    pub const fn can_resize(self, pointer_exists: bool, old_size: usize, new_size: usize) -> bool {
        if new_size == 0 {
            return true;
        }
        let tracked_old_size = if pointer_exists { old_size } else { 0 };
        if new_size <= tracked_old_size {
            return true;
        }
        let delta = new_size - tracked_old_size;
        delta <= self.limit_bytes && self.used_bytes <= self.limit_bytes - delta
    }

    /// Commit bookkeeping after the native allocation succeeds.
    pub fn commit_resize(&mut self, pointer_existed: bool, old_size: usize, new_size: usize) {
        if new_size == 0 {
            if pointer_existed {
                self.used_bytes = self.used_bytes.saturating_sub(old_size.min(self.used_bytes));
            }
            return;
        }
        let tracked_old_size = if pointer_existed { old_size } else { 0 };
        if new_size > tracked_old_size {
            self.used_bytes = self.used_bytes.saturating_add(new_size - tracked_old_size);
        } else {
            self.used_bytes =
                self.used_bytes.saturating_sub((tracked_old_size - new_size).min(self.used_bytes));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_memory_limit_clamps_to_cpp_bounds() {
        assert_eq!(memory_limit_bytes(None), 64 * 1024 * 1024);
        assert_eq!(memory_limit_bytes(Some(0)), 16 * 1024 * 1024);
        assert_eq!(memory_limit_bytes(Some(100)), 100 * 1024 * 1024);
        assert_eq!(memory_limit_bytes(Some(usize::MAX)), 256 * 1024 * 1024);
    }

    #[test]
    fn allocation_growth_is_rejected_before_limit_overflow() {
        let mut budget = LuaMemoryBudget::new(Some(16));
        let megabyte = 1024 * 1024;
        assert!(budget.can_resize(false, 123, 15 * megabyte));
        budget.commit_resize(false, 123, 15 * megabyte);
        assert_eq!(budget.used_bytes(), 15 * megabyte);
        assert!(!budget.can_resize(true, 15 * megabyte, 17 * megabyte));
        assert!(budget.can_resize(true, 15 * megabyte, 16 * megabyte));
    }

    #[test]
    fn shrink_and_free_match_cpp_saturating_accounting() {
        let mut budget = LuaMemoryBudget::new(None);
        budget.commit_resize(false, 0, 100);
        budget.commit_resize(true, 100, 40);
        assert_eq!(budget.used_bytes(), 40);
        budget.commit_resize(true, 1_000, 0);
        assert_eq!(budget.used_bytes(), 0);
    }
}
