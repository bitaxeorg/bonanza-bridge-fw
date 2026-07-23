//! Timing constants and small calculations used while servicing the Bridge
//! safety interlock.
//!
//! This module does not supervise hardware or own safety state. The pure state
//! machine lives in `safety`, while the UART control task samples inputs,
//! applies outputs, and feeds the hardware watchdog.

pub const MIN_LEASE_TIMEOUT_MS: u64 = 250;
pub const WATCHDOG_TIMEOUT_MS: u64 = 3_000;
pub const CONTROL_WRITE_TIMEOUT_MS: u64 = 100;
pub const MAX_SAFETY_SERVICE_INTERVAL_MS: u64 = 10;
pub const TACH_MEASUREMENT_MS: u64 = 500;
pub const TACH_SAMPLE_INTERVAL_US: u64 = 100;

pub const fn safety_service_due(elapsed_us: u64) -> bool {
    elapsed_us >= MAX_SAFETY_SERVICE_INTERVAL_MS * 1_000
}

pub const fn fan_rpm_from_half_second_pulses(pulse_count: u32) -> Option<u16> {
    let Some(rpm) = pulse_count.checked_mul(60) else {
        return None;
    };
    if rpm > u16::MAX as u32 {
        None
    } else {
        Some(rpm as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safety_service_interval_is_well_inside_the_minimum_lease() {
        assert!(MAX_SAFETY_SERVICE_INTERVAL_MS < MIN_LEASE_TIMEOUT_MS);
        assert!(CONTROL_WRITE_TIMEOUT_MS < MIN_LEASE_TIMEOUT_MS);
        assert!(TACH_MEASUREMENT_MS > MIN_LEASE_TIMEOUT_MS);
        assert!(TACH_MEASUREMENT_MS / MAX_SAFETY_SERVICE_INTERVAL_MS >= 50);
    }

    #[test]
    fn safety_service_deadline_has_an_exact_boundary() {
        assert!(!safety_service_due(9_999));
        assert!(safety_service_due(10_000));
    }

    #[test]
    fn tach_conversion_is_exact_and_rejects_wire_overflow() {
        assert_eq!(fan_rpm_from_half_second_pulses(0), Some(0));
        assert_eq!(fan_rpm_from_half_second_pulses(60), Some(3_600));
        assert_eq!(fan_rpm_from_half_second_pulses(1_092), Some(65_520));
        assert_eq!(fan_rpm_from_half_second_pulses(1_093), None);
        assert_eq!(fan_rpm_from_half_second_pulses(u32::MAX), None);
    }
}
