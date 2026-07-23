use embassy_rp::{gpio, pwm};
use embassy_time::{Duration, Instant, Timer};

use bonanza_bridge_fw::{
    safety::fan_pwm_compare,
    safety_timing::{fan_rpm_from_half_second_pulses, safety_service_due, TACH_MEASUREMENT_MS, TACH_SAMPLE_INTERVAL_US},
};

use super::{CommandError, Controller, ControllerCommand};

pub struct Pins<'d> {
    pub pwm: pwm::Pwm<'d>,
    pub tach: gpio::Input<'d>,
}

pub fn pwm_config_for_percent(percent: u8) -> pwm::Config {
    let mut config = pwm::Config::default();
    config.top = 1000;
    config.compare_a = fan_pwm_compare(percent);
    config.compare_b = 0;
    config.divider = 5.into();
    // The board's standard fan-control path is active-low: an effective
    // 100-percent request holds the PWM output low for full speed.
    config.invert_a = true;
    config.phase_correct = false;
    config.enable = true;
    config
}

pub fn apply_percent(pwm: &mut pwm::Pwm<'_>, percent: u8) {
    pwm.set_config(&pwm_config_for_percent(percent));
}

#[derive(defmt::Format)]
pub enum Command {
    SetSpeed(u8), // 0-100 percent
    GetTach,
}

impl Command {
    pub fn from_bytes(buf: &[u8]) -> Result<Self, CommandError> {
        match buf[0] {
            0x10 => {
                if buf.len() < 2 {
                    return Err(CommandError::Invalid);
                }
                let speed = buf[1];
                if speed > 100 {
                    return Err(CommandError::Invalid);
                }
                Ok(Command::SetSpeed(speed))
            }
            0x20 => Ok(Command::GetTach),
            _ => Err(CommandError::Invalid),
        }
    }
}

impl ControllerCommand for Command {
    async fn handle(&self, controller: &mut Controller) -> Result<heapless::Vec<u8, 256>, CommandError> {
        match self {
            Command::SetSpeed(speed) => {
                controller.safety_request_fan_percent(*speed).map_err(CommandError::from)?;

                let mut res = heapless::Vec::new();
                res.push(0x00).unwrap(); // Success
                Ok(res)
            }
            Command::GetTach => {
                // Measure fan tachometer pulses
                // Most fans output 2 pulses per revolution
                let rpm = measure_fan_rpm(controller).await?;

                let mut res = heapless::Vec::new();
                res.extend_from_slice(&rpm.to_le_bytes()).unwrap();
                Ok(res)
            }
        }
    }
}

/// Measure fan RPM by counting tachometer pulses over a period
/// Most PC fans output 2 pulses per revolution
async fn measure_fan_rpm(controller: &mut Controller) -> Result<u16, CommandError> {
    let measurement_time = Duration::from_millis(TACH_MEASUREMENT_MS);
    let start = Instant::now();
    let mut last_safety_service = Instant::now();
    let mut pulse_count = 0u32;
    let mut last_state = controller.fan.tach.is_high();

    // Count rising edges for 500ms
    while start.elapsed() < measurement_time {
        let current_state = controller.fan.tach.is_high();
        if current_state && !last_state {
            pulse_count += 1;
        }
        last_state = current_state;
        if safety_service_due(last_safety_service.elapsed().as_micros()) {
            controller.service_safety();
            last_safety_service = Instant::now();
        }
        Timer::after_micros(TACH_SAMPLE_INTERVAL_US).await; // Sample at 10kHz
    }

    // Calculate RPM: (pulses / 2) * (60 / 0.5) = pulses * 60
    // Most fans have 2 pulses per revolution
    controller.service_safety();
    fan_rpm_from_half_second_pulses(pulse_count).ok_or(CommandError::Invalid)
}
