#[cfg(target_os = "windows")]
use smu_scripting::ScriptLogSink;

fn requested_timer_period(period_min: u32, period_max: u32) -> u32 {
    let mut requested = 1_u32.max(period_min);
    if period_max > 0 {
        requested = requested.min(period_max);
    }
    requested
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::ffi::c_void;
    use std::mem::size_of;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Media::{
        MMSYSERR_NOERROR, TIMECAPS, TIMERR_NOERROR, timeBeginPeriod, timeEndPeriod, timeGetDevCaps,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        PROCESS_POWER_THROTTLING_STATE, ProcessPowerThrottling, SetProcessInformation,
    };

    const EXECUTION_SPEED_ONLY: u32 = PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
    const FULL_CONTROL_MASK: u32 =
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION;

    fn logger() -> std::sync::Arc<crate::script_logging::AppScriptLogger> {
        crate::script_logging::shared()
    }

    fn log_info(message: impl AsRef<[u8]>) {
        logger().log_info(message.as_ref());
    }

    fn log_warning(message: impl AsRef<[u8]>) {
        logger().log_warning(message.as_ref());
    }

    fn configure_timer_period() -> Option<u32> {
        let mut caps = TIMECAPS::default();
        let mut requested = 1_u32;
        // SAFETY: `caps` is valid writable storage for TIMECAPS and the byte size matches the struct.
        if unsafe { timeGetDevCaps(&raw mut caps, size_of::<TIMECAPS>() as u32) }
            == MMSYSERR_NOERROR
        {
            requested = requested_timer_period(caps.wPeriodMin, caps.wPeriodMax);
        }

        // SAFETY: timeBeginPeriod accepts any UINT period and owns no borrowed memory.
        if unsafe { timeBeginPeriod(requested) } == TIMERR_NOERROR {
            log_info("Requested high-resolution Windows timer period for macro/script timing.");
            Some(requested)
        } else {
            log_warning(
                "Windows refused the high-resolution timer-period request; short script sleeps may fall back to default scheduler granularity.",
            );
            None
        }
    }

    fn set_power_throttling(control_mask: u32) -> Result<(), u32> {
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: control_mask,
            StateMask: 0,
        };
        // SAFETY: GetCurrentProcess returns a pseudo-handle valid in this process. `state` remains alive for the
        // synchronous SetProcessInformation call and its byte size matches PROCESS_POWER_THROTTLING_STATE.
        let result = unsafe {
            SetProcessInformation(
                GetCurrentProcess(),
                ProcessPowerThrottling,
                (&raw const state).cast::<c_void>(),
                size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
        };
        if result != 0 {
            Ok(())
        } else {
            // SAFETY: GetLastError has no preconditions and reads this thread's last Win32 error value.
            Err(unsafe { GetLastError() })
        }
    }

    fn configure_power_throttling() {
        match set_power_throttling(FULL_CONTROL_MASK) {
            Ok(()) => {}
            Err(first_error) => match set_power_throttling(EXECUTION_SPEED_ONLY) {
                Ok(()) => {
                    log_info(format!(
                        "Windows does not support the ignore-timer-resolution power-throttling flag; using the compatible execution-speed-only policy. Error={first_error}."
                    ));
                }
                Err(retry_error) => {
                    log_warning(format!(
                        "Windows refused process power-throttling configuration; first error={first_error}, retry error={retry_error}."
                    ));
                }
            },
        }
    }

    #[derive(Debug)]
    pub struct WindowsTimingGuard {
        timer_resolution_period: Option<u32>,
    }

    impl WindowsTimingGuard {
        #[must_use]
        pub fn configure() -> Self {
            let timer_resolution_period = configure_timer_period();
            configure_power_throttling();
            Self { timer_resolution_period }
        }
    }

    impl Drop for WindowsTimingGuard {
        fn drop(&mut self) {
            if let Some(period) = self.timer_resolution_period.take() {
                // SAFETY: this balances the successful timeBeginPeriod(period) performed by this guard.
                let _ = unsafe { timeEndPeriod(period) };
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub use platform::WindowsTimingGuard;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_period_matches_cpp_clamp_policy() {
        assert_eq!(requested_timer_period(1, 15), 1);
        assert_eq!(requested_timer_period(5, 15), 5);
        assert_eq!(requested_timer_period(0, 0), 1);
        assert_eq!(requested_timer_period(0, 1), 1);
        assert_eq!(requested_timer_period(4, 2), 2);
    }
}
