use heapless::Vec;

pub const SAFETY_STATUS_SCHEMA_VERSION: u8 = 1;
pub const SAFETY_STATUS_ENCODED_LEN: usize = 17;
pub const PRODUCTION_LEASE_TIMEOUT_MS: u32 = 2_000;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SafetyWireCommand {
    GetStatus = 0x10,
    ArmLease = 0x11,
    Heartbeat = 0x12,
    ClearFault = 0x13,
    Disarm = 0x14,
}

pub fn decode_wire_command(bytes: &[u8]) -> Option<SafetyWireCommand> {
    match bytes {
        [0x10] => Some(SafetyWireCommand::GetStatus),
        [0x11] => Some(SafetyWireCommand::ArmLease),
        [0x12] => Some(SafetyWireCommand::Heartbeat),
        [0x13] => Some(SafetyWireCommand::ClearFault),
        [0x14] => Some(SafetyWireCommand::Disarm),
        _ => None,
    }
}

pub const CAP_FIVE_VOLT_CONTROL: u16 = 1 << 0;
pub const CAP_ASIC_RESET_CONTROL: u16 = 1 << 1;
pub const CAP_FAN_FORCE_FULL: u16 = 1 << 2;
pub const CAP_TRIP_INPUT_SAMPLED: u16 = 1 << 3;
pub const CAP_CORE_POWER_CUTOFF: u16 = 1 << 4;
pub const CAP_FAN_TACH_INTERLOCK: u16 = 1 << 5;
pub const CAP_INDEPENDENT_TRIP_MONITOR: u16 = 1 << 6;

pub const EVIDENCE_OUTPUTS_SAFE: u16 = 1 << 0;
pub const EVIDENCE_LEASE_VALID: u16 = 1 << 1;
pub const EVIDENCE_TRIP_CLEAR: u16 = 1 << 2;
pub const EVIDENCE_FAULT_CLEAR: u16 = 1 << 3;
pub const EVIDENCE_CORE_CUTOFF_AVAILABLE: u16 = 1 << 4;
pub const EVIDENCE_FAN_TACH_INTERLOCK_AVAILABLE: u16 = 1 << 5;
pub const EVIDENCE_INDEPENDENT_TRIP_MONITOR_AVAILABLE: u16 = 1 << 6;

const REQUIRED_PRODUCTION_CAPABILITIES: u16 = CAP_FIVE_VOLT_CONTROL | CAP_ASIC_RESET_CONTROL | CAP_FAN_FORCE_FULL | CAP_TRIP_INPUT_SAMPLED | CAP_CORE_POWER_CUTOFF | CAP_FAN_TACH_INTERLOCK | CAP_INDEPENDENT_TRIP_MONITOR;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SafetyStage {
    BootSafe = 0,
    Lease = 1,
    TripLatch = 2,
}

impl SafetyStage {
    pub const fn enforces_lease(self) -> bool {
        matches!(self, Self::Lease | Self::TripLatch)
    }

    pub const fn enforces_trip_latch(self) -> bool {
        matches!(self, Self::TripLatch)
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SafetyState {
    SafeOff = 0,
    Controlled = 1,
    FaultLatched = 2,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FaultReason {
    None = 0,
    LeaseExpired = 1,
    AsicTrip = 2,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeVerdict {
    GoodSafeOff = 0,
    GoodControlled = 1,
    BadFault = 0x80,
    BadLease = 0x81,
    BadTripInput = 0x82,
    BadUnsafeOutputs = 0x83,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProductionVerdict {
    Good = 0,
    BadStageDisabled = 0x80,
    BadCapabilityGap = 0x81,
    BadRuntime = 0x82,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SafetyError {
    LeaseRequired,
    LeaseExpired,
    FaultLatched,
    TripActive,
    InvalidSequence,
    FanNotSafe,
    InvalidFanPercent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SafetyConfig {
    pub stage: SafetyStage,
    pub lease_timeout_ms: u32,
    pub capabilities: u16,
}

impl SafetyConfig {
    pub const fn firmware() -> Self {
        Self {
            // Production images always enforce both the local output lease
            // and the active-high trip latch. Qualification-only build modes
            // must not be able to weaken this policy.
            stage: SafetyStage::TripLatch,
            lease_timeout_ms: PRODUCTION_LEASE_TIMEOUT_MS,
            // These describe only the control paths implemented by this firmware.
            // Independent VCORE cutoff, tach interlock, and independent trip
            // monitoring are deliberately absent until hardware proves them.
            capabilities: CAP_FIVE_VOLT_CONTROL | CAP_ASIC_RESET_CONTROL | CAP_FAN_FORCE_FULL | CAP_TRIP_INPUT_SAMPLED,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SafetyOutputs {
    pub five_volt_enabled: bool,
    pub asic_reset_asserted: bool,
    pub fan_percent: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoardControlIntent {
    pub five_volt_enable_high: bool,
    pub asic_reset_n_high: bool,
    pub fan_percent: u8,
}

impl SafetyOutputs {
    pub const SAFE: Self = Self {
        five_volt_enabled: false,
        asic_reset_asserted: true,
        fan_percent: 100,
    };

    pub const fn is_safe(self) -> bool {
        !self.five_volt_enabled && self.asic_reset_asserted && self.fan_percent == 100
    }

    pub const fn board_control_intent(self) -> BoardControlIntent {
        BoardControlIntent {
            five_volt_enable_high: self.five_volt_enabled,
            asic_reset_n_high: !self.asic_reset_asserted,
            fan_percent: self.fan_percent,
        }
    }
}

pub const fn fan_pwm_compare(percent: u8) -> u16 {
    let bounded_percent = if percent > 100 { 100 } else { percent };
    (1000u32 * bounded_percent as u32 / 100) as u16
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SafetyStatus {
    pub stage: SafetyStage,
    pub state: SafetyState,
    pub fault: FaultReason,
    pub runtime_verdict: RuntimeVerdict,
    pub production_verdict: ProductionVerdict,
    pub capabilities: u16,
    pub evidence: u16,
    pub lease_remaining_ms: u32,
    pub outputs: SafetyOutputs,
    pub trip_input_asserted: bool,
}

impl SafetyStatus {
    pub fn encode(self) -> Vec<u8, 32> {
        let mut payload = Vec::new();
        payload
            .extend_from_slice(&[SAFETY_STATUS_SCHEMA_VERSION, self.stage as u8, self.state as u8, self.fault as u8, self.runtime_verdict as u8, self.production_verdict as u8])
            .unwrap();
        payload.extend_from_slice(&self.capabilities.to_le_bytes()).unwrap();
        payload.extend_from_slice(&self.evidence.to_le_bytes()).unwrap();
        payload.extend_from_slice(&self.lease_remaining_ms.to_le_bytes()).unwrap();

        let output_flags = u8::from(self.outputs.five_volt_enabled) | (u8::from(self.outputs.asic_reset_asserted) << 1) | (u8::from(self.outputs.fan_percent == 100) << 2);
        payload.extend_from_slice(&[output_flags, self.outputs.fan_percent, u8::from(self.trip_input_asserted)]).unwrap();
        debug_assert_eq!(payload.len(), SAFETY_STATUS_ENCODED_LEN);
        payload
    }
}

#[derive(Debug)]
pub struct SafetyPolicy {
    config: SafetyConfig,
    state: SafetyState,
    fault: FaultReason,
    requested_outputs: SafetyOutputs,
    trip_input_asserted: bool,
    lease_deadline_ms: Option<u64>,
}

impl SafetyPolicy {
    pub const fn new(config: SafetyConfig) -> Self {
        Self {
            config,
            state: SafetyState::SafeOff,
            fault: FaultReason::None,
            requested_outputs: SafetyOutputs::SAFE,
            trip_input_asserted: false,
            lease_deadline_ms: None,
        }
    }

    pub const fn config(&self) -> SafetyConfig {
        self.config
    }

    pub const fn outputs(&self) -> SafetyOutputs {
        if matches!(self.state, SafetyState::FaultLatched) {
            SafetyOutputs::SAFE
        } else {
            self.requested_outputs
        }
    }

    pub fn tick(&mut self, now_ms: u64, trip_input_asserted: bool) {
        self.trip_input_asserted = trip_input_asserted;

        if !matches!(self.state, SafetyState::FaultLatched) && self.config.stage.enforces_trip_latch() && trip_input_asserted {
            self.latch_fault(FaultReason::AsicTrip);
            return;
        }

        if !matches!(self.state, SafetyState::Controlled) || !self.config.stage.enforces_lease() {
            return;
        }

        if self.lease_deadline_ms.is_some_and(|deadline| now_ms >= deadline) {
            self.latch_fault(FaultReason::LeaseExpired);
        }
    }

    pub fn arm(&mut self, now_ms: u64) -> Result<(), SafetyError> {
        self.tick(now_ms, self.trip_input_asserted);
        self.ensure_not_faulted()?;
        if self.config.stage.enforces_trip_latch() && self.trip_input_asserted {
            return Err(SafetyError::TripActive);
        }

        self.requested_outputs = SafetyOutputs::SAFE;
        self.state = SafetyState::Controlled;
        self.lease_deadline_ms = self.config.stage.enforces_lease().then_some(now_ms.saturating_add(self.config.lease_timeout_ms as u64));
        Ok(())
    }

    pub fn heartbeat(&mut self, now_ms: u64) -> Result<(), SafetyError> {
        self.tick(now_ms, self.trip_input_asserted);
        self.ensure_not_faulted()?;
        if !matches!(self.state, SafetyState::Controlled) {
            return Err(SafetyError::LeaseRequired);
        }

        if self.config.stage.enforces_lease() {
            self.lease_deadline_ms = Some(now_ms.saturating_add(self.config.lease_timeout_ms as u64));
        }
        Ok(())
    }

    pub fn request_five_volt_enabled(&mut self, enabled: bool, now_ms: u64) -> Result<(), SafetyError> {
        self.tick(now_ms, self.trip_input_asserted);
        if !enabled {
            self.requested_outputs.five_volt_enabled = false;
            self.requested_outputs.asic_reset_asserted = true;
            self.requested_outputs.fan_percent = 100;
            return Ok(());
        }

        self.ensure_controlled(now_ms)?;
        if self.config.stage.enforces_lease() {
            if self.requested_outputs.fan_percent != 100 {
                return Err(SafetyError::FanNotSafe);
            }
            if !self.requested_outputs.asic_reset_asserted {
                return Err(SafetyError::InvalidSequence);
            }
        }
        self.requested_outputs.five_volt_enabled = true;
        Ok(())
    }

    pub fn request_asic_reset_asserted(&mut self, asserted: bool, now_ms: u64) -> Result<(), SafetyError> {
        self.tick(now_ms, self.trip_input_asserted);
        if asserted {
            self.requested_outputs.asic_reset_asserted = true;
            return Ok(());
        }

        self.ensure_controlled(now_ms)?;
        if self.config.stage.enforces_lease() && !self.requested_outputs.five_volt_enabled {
            return Err(SafetyError::InvalidSequence);
        }
        self.requested_outputs.asic_reset_asserted = false;
        Ok(())
    }

    pub fn request_fan_percent(&mut self, percent: u8, now_ms: u64) -> Result<(), SafetyError> {
        self.tick(now_ms, self.trip_input_asserted);
        if percent > 100 {
            return Err(SafetyError::InvalidFanPercent);
        }
        if percent == 100 {
            self.requested_outputs.fan_percent = percent;
            return Ok(());
        }

        self.ensure_controlled(now_ms)?;
        if self.config.stage.enforces_lease() && self.requested_outputs.five_volt_enabled {
            return Err(SafetyError::InvalidSequence);
        }
        self.requested_outputs.fan_percent = percent;
        Ok(())
    }

    pub fn clear_fault(&mut self, now_ms: u64) -> Result<(), SafetyError> {
        if self.trip_input_asserted {
            return Err(SafetyError::TripActive);
        }

        self.state = SafetyState::SafeOff;
        self.fault = FaultReason::None;
        self.requested_outputs = SafetyOutputs::SAFE;
        self.lease_deadline_ms = None;
        self.tick(now_ms, false);
        Ok(())
    }

    pub fn disarm(&mut self) {
        self.requested_outputs = SafetyOutputs::SAFE;
        self.lease_deadline_ms = None;
        if matches!(self.fault, FaultReason::None) {
            self.state = SafetyState::SafeOff;
        }
    }

    pub fn status(&self, now_ms: u64) -> SafetyStatus {
        let outputs = self.outputs();
        let lease_remaining_ms = self.lease_deadline_ms.map(|deadline| deadline.saturating_sub(now_ms).min(u32::MAX as u64) as u32).unwrap_or(0);
        let lease_valid = !self.config.stage.enforces_lease() || (matches!(self.state, SafetyState::Controlled) && lease_remaining_ms > 0);

        let mut evidence = 0;
        if outputs.is_safe() {
            evidence |= EVIDENCE_OUTPUTS_SAFE;
        }
        if lease_valid {
            evidence |= EVIDENCE_LEASE_VALID;
        }
        if !self.trip_input_asserted {
            evidence |= EVIDENCE_TRIP_CLEAR;
        }
        if matches!(self.fault, FaultReason::None) {
            evidence |= EVIDENCE_FAULT_CLEAR;
        }
        if self.config.capabilities & CAP_CORE_POWER_CUTOFF != 0 {
            evidence |= EVIDENCE_CORE_CUTOFF_AVAILABLE;
        }
        if self.config.capabilities & CAP_FAN_TACH_INTERLOCK != 0 {
            evidence |= EVIDENCE_FAN_TACH_INTERLOCK_AVAILABLE;
        }
        if self.config.capabilities & CAP_INDEPENDENT_TRIP_MONITOR != 0 {
            evidence |= EVIDENCE_INDEPENDENT_TRIP_MONITOR_AVAILABLE;
        }

        let runtime_verdict = if !matches!(self.fault, FaultReason::None) {
            RuntimeVerdict::BadFault
        } else if self.trip_input_asserted {
            RuntimeVerdict::BadTripInput
        } else if matches!(self.state, SafetyState::SafeOff) {
            if outputs.is_safe() {
                RuntimeVerdict::GoodSafeOff
            } else {
                RuntimeVerdict::BadUnsafeOutputs
            }
        } else if self.config.stage.enforces_lease() && !lease_valid {
            RuntimeVerdict::BadLease
        } else {
            RuntimeVerdict::GoodControlled
        };

        let production_verdict = if !self.config.stage.enforces_trip_latch() {
            ProductionVerdict::BadStageDisabled
        } else if !matches!(runtime_verdict, RuntimeVerdict::GoodSafeOff | RuntimeVerdict::GoodControlled) {
            ProductionVerdict::BadRuntime
        } else if self.config.capabilities & REQUIRED_PRODUCTION_CAPABILITIES != REQUIRED_PRODUCTION_CAPABILITIES {
            ProductionVerdict::BadCapabilityGap
        } else {
            ProductionVerdict::Good
        };

        SafetyStatus {
            stage: self.config.stage,
            state: self.state,
            fault: self.fault,
            runtime_verdict,
            production_verdict,
            capabilities: self.config.capabilities,
            evidence,
            lease_remaining_ms,
            outputs,
            trip_input_asserted: self.trip_input_asserted,
        }
    }

    fn ensure_controlled(&mut self, now_ms: u64) -> Result<(), SafetyError> {
        self.ensure_not_faulted()?;
        if matches!(self.state, SafetyState::SafeOff) {
            if self.config.stage.enforces_lease() {
                return Err(SafetyError::LeaseRequired);
            }
            self.state = SafetyState::Controlled;
        }
        if self.config.stage.enforces_lease() && self.lease_deadline_ms.is_none_or(|deadline| now_ms >= deadline) {
            self.latch_fault(FaultReason::LeaseExpired);
            return Err(SafetyError::LeaseExpired);
        }
        Ok(())
    }

    fn ensure_not_faulted(&self) -> Result<(), SafetyError> {
        if matches!(self.state, SafetyState::FaultLatched) {
            Err(SafetyError::FaultLatched)
        } else {
            Ok(())
        }
    }

    fn latch_fault(&mut self, reason: FaultReason) {
        self.state = SafetyState::FaultLatched;
        self.fault = reason;
        self.requested_outputs = SafetyOutputs::SAFE;
        self.lease_deadline_ms = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMPLEMENTED_CAPABILITIES: u16 = CAP_FIVE_VOLT_CONTROL | CAP_ASIC_RESET_CONTROL | CAP_FAN_FORCE_FULL | CAP_TRIP_INPUT_SAMPLED;

    fn config(stage: SafetyStage) -> SafetyConfig {
        SafetyConfig {
            stage,
            lease_timeout_ms: 1_000,
            capabilities: IMPLEMENTED_CAPABILITIES,
        }
    }

    #[test]
    fn every_stage_starts_with_semantic_safe_outputs() {
        for stage in [SafetyStage::BootSafe, SafetyStage::Lease, SafetyStage::TripLatch] {
            let policy = SafetyPolicy::new(config(stage));
            let status = policy.status(0);

            assert_eq!(status.state, SafetyState::SafeOff);
            assert_eq!(status.outputs, SafetyOutputs::SAFE);
            assert_eq!(status.runtime_verdict, RuntimeVerdict::GoodSafeOff);
            assert_ne!(status.evidence & EVIDENCE_OUTPUTS_SAFE, 0);
        }
    }

    #[test]
    fn safe_semantics_map_to_low_enable_low_reset_n_and_full_fan() {
        let intent = SafetyOutputs::SAFE.board_control_intent();

        assert!(!intent.five_volt_enable_high);
        assert!(!intent.asic_reset_n_high);
        assert_eq!(intent.fan_percent, 100);
        assert_eq!(fan_pwm_compare(intent.fan_percent), 1000);
        assert_eq!(fan_pwm_compare(0), 0);
        assert_eq!(fan_pwm_compare(101), 1000);
    }

    #[test]
    fn firmware_configuration_is_fixed_and_does_not_claim_hardware_gates() {
        let config = SafetyConfig::firmware();

        assert_eq!(config.stage, SafetyStage::TripLatch);
        assert_eq!(config.lease_timeout_ms, PRODUCTION_LEASE_TIMEOUT_MS);
        assert_eq!(config.capabilities, IMPLEMENTED_CAPABILITIES);
        assert_eq!(config.capabilities & REQUIRED_PRODUCTION_CAPABILITIES, IMPLEMENTED_CAPABILITIES);
    }

    #[test]
    fn boot_safe_stage_retains_legacy_control_after_safe_boot() {
        let mut policy = SafetyPolicy::new(config(SafetyStage::BootSafe));

        policy.request_asic_reset_asserted(false, 0).unwrap();
        policy.request_fan_percent(50, 0).unwrap();
        policy.request_five_volt_enabled(true, 0).unwrap();
        assert_eq!(
            policy.outputs(),
            SafetyOutputs {
                five_volt_enabled: true,
                asic_reset_asserted: false,
                fan_percent: 50,
            }
        );

        policy.request_five_volt_enabled(false, 1).unwrap();
        assert_eq!(policy.outputs(), SafetyOutputs::SAFE);
    }

    #[test]
    fn lease_stage_rejects_unsafe_changes_until_armed() {
        let mut policy = SafetyPolicy::new(config(SafetyStage::Lease));

        assert_eq!(policy.request_five_volt_enabled(true, 1), Err(SafetyError::LeaseRequired));
        policy.arm(10).unwrap();
        assert_eq!(policy.status(10).lease_remaining_ms, 1_000);
        policy.request_five_volt_enabled(true, 10).unwrap();
        policy.request_asic_reset_asserted(false, 10).unwrap();
        assert_eq!(policy.status(10).runtime_verdict, RuntimeVerdict::GoodControlled);
    }

    #[test]
    fn heartbeat_extends_the_lease_and_expiry_latches_safe_outputs() {
        let mut policy = SafetyPolicy::new(config(SafetyStage::Lease));
        policy.arm(100).unwrap();
        policy.request_five_volt_enabled(true, 100).unwrap();
        policy.request_asic_reset_asserted(false, 100).unwrap();

        policy.heartbeat(900).unwrap();
        policy.tick(1_899, false);
        assert_eq!(policy.status(1_899).state, SafetyState::Controlled);

        policy.tick(1_900, false);
        let status = policy.status(1_900);
        assert_eq!(status.state, SafetyState::FaultLatched);
        assert_eq!(status.fault, FaultReason::LeaseExpired);
        assert_eq!(status.outputs, SafetyOutputs::SAFE);
        assert_eq!(status.runtime_verdict, RuntimeVerdict::BadFault);
        assert_eq!(policy.heartbeat(1_901), Err(SafetyError::FaultLatched));
    }

    #[test]
    fn disarm_is_a_clean_safe_off_but_does_not_erase_a_fault() {
        let mut policy = SafetyPolicy::new(config(SafetyStage::Lease));
        policy.arm(0).unwrap();
        policy.request_five_volt_enabled(true, 0).unwrap();
        policy.request_asic_reset_asserted(false, 0).unwrap();

        policy.disarm();
        let status = policy.status(1);
        assert_eq!(status.state, SafetyState::SafeOff);
        assert_eq!(status.fault, FaultReason::None);
        assert_eq!(status.outputs, SafetyOutputs::SAFE);
        assert_eq!(policy.heartbeat(1), Err(SafetyError::LeaseRequired));

        policy.arm(2).unwrap();
        policy.tick(1_002, false);
        policy.disarm();
        assert_eq!(policy.status(1_003).state, SafetyState::FaultLatched);
        assert_eq!(policy.status(1_003).fault, FaultReason::LeaseExpired);
    }

    #[test]
    fn trip_is_visible_before_enforcement_and_latched_at_stage_two() {
        let mut observe_only = SafetyPolicy::new(config(SafetyStage::Lease));
        observe_only.tick(0, true);
        assert_eq!(observe_only.status(0).fault, FaultReason::None);
        assert_eq!(observe_only.status(0).runtime_verdict, RuntimeVerdict::BadTripInput);

        let mut enforced = SafetyPolicy::new(config(SafetyStage::TripLatch));
        enforced.arm(0).unwrap();
        enforced.request_five_volt_enabled(true, 0).unwrap();
        enforced.tick(1, true);
        let status = enforced.status(1);
        assert_eq!(status.fault, FaultReason::AsicTrip);
        assert_eq!(status.outputs, SafetyOutputs::SAFE);
        assert_eq!(enforced.clear_fault(2), Err(SafetyError::TripActive));

        enforced.tick(3, false);
        enforced.clear_fault(3).unwrap();
        assert_eq!(enforced.status(3).state, SafetyState::SafeOff);
        assert_eq!(enforced.status(3).outputs, SafetyOutputs::SAFE);
    }

    #[test]
    fn safe_requests_remain_available_while_fault_latched() {
        let mut policy = SafetyPolicy::new(config(SafetyStage::Lease));
        policy.arm(0).unwrap();
        policy.request_five_volt_enabled(true, 0).unwrap();
        policy.tick(1_000, false);

        policy.request_asic_reset_asserted(true, 1_001).unwrap();
        policy.request_five_volt_enabled(false, 1_001).unwrap();
        policy.request_fan_percent(100, 1_001).unwrap();
        assert_eq!(policy.outputs(), SafetyOutputs::SAFE);
    }

    #[test]
    fn full_fan_is_required_while_five_volt_output_is_enabled() {
        let mut policy = SafetyPolicy::new(config(SafetyStage::Lease));

        policy.arm(0).unwrap();
        policy.request_fan_percent(50, 0).unwrap();
        assert_eq!(policy.request_five_volt_enabled(true, 0), Err(SafetyError::FanNotSafe));
        policy.request_fan_percent(100, 0).unwrap();
        policy.request_five_volt_enabled(true, 0).unwrap();
        assert_eq!(policy.request_fan_percent(99, 0), Err(SafetyError::InvalidSequence));
    }

    #[test]
    fn current_firmware_capabilities_are_explicitly_not_production_ready() {
        let policy = SafetyPolicy::new(config(SafetyStage::TripLatch));
        let status = policy.status(0);

        assert_eq!(status.production_verdict, ProductionVerdict::BadCapabilityGap);
        assert_eq!(status.capabilities & CAP_CORE_POWER_CUTOFF, 0);
        assert_eq!(status.capabilities & CAP_FAN_TACH_INTERLOCK, 0);
        assert_eq!(status.capabilities & CAP_INDEPENDENT_TRIP_MONITOR, 0);
    }

    #[test]
    fn production_verdict_requires_stage_capabilities_and_good_runtime() {
        let ready_config = SafetyConfig {
            stage: SafetyStage::TripLatch,
            lease_timeout_ms: 1_000,
            capabilities: REQUIRED_PRODUCTION_CAPABILITIES,
        };
        let mut policy = SafetyPolicy::new(ready_config);
        assert_eq!(policy.status(0).production_verdict, ProductionVerdict::Good);

        policy.tick(1, true);
        assert_eq!(policy.status(1).production_verdict, ProductionVerdict::BadRuntime);
    }

    #[test]
    fn safety_status_encoding_is_fixed_and_self_describing() {
        let mut policy = SafetyPolicy::new(config(SafetyStage::Lease));
        policy.arm(100).unwrap();
        let status = policy.status(125);
        let encoded = status.encode();

        assert_eq!(encoded.len(), SAFETY_STATUS_ENCODED_LEN);
        assert_eq!(encoded[0], SAFETY_STATUS_SCHEMA_VERSION);
        assert_eq!(encoded[1], SafetyStage::Lease as u8);
        assert_eq!(encoded[2], SafetyState::Controlled as u8);
        assert_eq!(encoded[3], FaultReason::None as u8);
        assert_eq!(encoded[4], RuntimeVerdict::GoodControlled as u8);
        assert_eq!(encoded[5], ProductionVerdict::BadStageDisabled as u8);
        assert_eq!(u16::from_le_bytes([encoded[6], encoded[7]]), IMPLEMENTED_CAPABILITIES);
        assert_eq!(u32::from_le_bytes(encoded[10..14].try_into().unwrap()), 975);
        assert_eq!(encoded[14], 0b0000_0110);
        assert_eq!(encoded[15], 100);
        assert_eq!(encoded[16], 0);
    }

    #[test]
    fn safety_wire_commands_are_exact_and_reject_trailing_data() {
        assert_eq!(decode_wire_command(&[0x10]), Some(SafetyWireCommand::GetStatus));
        assert_eq!(decode_wire_command(&[0x11]), Some(SafetyWireCommand::ArmLease));
        assert_eq!(decode_wire_command(&[0x12]), Some(SafetyWireCommand::Heartbeat));
        assert_eq!(decode_wire_command(&[0x13]), Some(SafetyWireCommand::ClearFault));
        assert_eq!(decode_wire_command(&[0x14]), Some(SafetyWireCommand::Disarm));
        assert_eq!(decode_wire_command(&[]), None);
        assert_eq!(decode_wire_command(&[0x10, 0x00]), None);
        assert_eq!(decode_wire_command(&[0xff]), None);
    }
}
