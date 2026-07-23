use defmt::{info, warn};

use bonanza_bridge_fw::{
    safety::{SafetyConfig, SafetyError, SafetyOutputs, SafetyPolicy},
    safety_timing::CONTROL_WRITE_TIMEOUT_MS,
};

use embassy_rp::{
    peripherals::UART0,
    uart::{BufferedUart, Error as UartError},
    watchdog::Watchdog,
};
use embassy_time::{Duration, Instant, TimeoutError};
use embedded_io_async::{Read, Write};
use heapless::Vec;

use crate::pio_uart::set_buffered_rx_forwarding_enabled;

pub mod system;
const SYSTEM_COMMAND: u8 = 0;

pub mod gpio;
const GPIO_COMMAND: u8 = 6;

pub mod fan;
const FAN_COMMAND: u8 = 9;

#[derive(defmt::Format)]
struct Command {
    id: u8,
    _bus: u8,
    inner: CommandInner,
}

#[derive(defmt::Format)]
enum CommandInner {
    System(system::Command),
    Gpio(gpio::Command),
    Fan(fan::Command),
}

impl Command {
    fn from_bytes(buf: &[u8]) -> Result<Self, CommandError> {
        let id = buf[0];
        match buf[2] {
            SYSTEM_COMMAND => Ok(Self {
                id,
                _bus: buf[1],
                inner: CommandInner::System(system::Command::from_bytes(&buf[3..])?),
            }),
            GPIO_COMMAND => Ok(Self {
                id,
                _bus: buf[1],
                inner: CommandInner::Gpio(gpio::Command::from_bytes(&buf[3..])?),
            }),
            FAN_COMMAND => Ok(Self {
                id,
                _bus: buf[1],
                inner: CommandInner::Fan(fan::Command::from_bytes(&buf[3..])?),
            }),
            _ => Err(CommandError::Invalid),
        }
    }
}

#[derive(defmt::Format)]
pub enum CommandError {
    Timeout, // 0x10
    Invalid, // 0x11
    Denied,  // 0x12
    Fault,   // 0x13
}

impl From<SafetyError> for CommandError {
    fn from(error: SafetyError) -> Self {
        match error {
            SafetyError::LeaseExpired | SafetyError::FaultLatched | SafetyError::TripActive => Self::Fault,
            SafetyError::LeaseRequired | SafetyError::InvalidSequence | SafetyError::FanNotSafe | SafetyError::InvalidFanPercent => Self::Denied,
        }
    }
}

impl CommandError {
    fn encode(&self, id: u8) -> Vec<u8, 260> {
        let mut buf = Vec::<u8, 260>::new();
        buf.extend_from_slice(&[0x00, 0x00, id]).unwrap();

        match self {
            CommandError::Timeout => {
                buf.push(0x10).unwrap();
            }
            CommandError::Invalid => {
                buf.push(0x11).unwrap();
            }
            CommandError::Denied => {
                buf.push(0x12).unwrap();
            }
            CommandError::Fault => {
                buf.push(0x13).unwrap();
            }
        }

        let len = (buf.len() as u16).to_le_bytes();
        buf[0..2].clone_from_slice(&len);
        buf
    }
}

enum ReadPacketResult {
    Command(Command),
    Error { id: u8, error: CommandError },
}

pub struct Controller {
    gpio: gpio::Pins<'static>,
    fan: fan::Pins<'static>,
    safety: SafetyPolicy,
    applied_outputs: Option<SafetyOutputs>,
    watchdog: Watchdog,
}

pub trait ControllerCommand {
    async fn handle(&self, controller: &mut Controller) -> Result<Vec<u8, 256>, CommandError>;
}

impl Controller {
    fn new(gpio: gpio::Pins<'static>, fan: fan::Pins<'static>, watchdog: Watchdog) -> Self {
        Self {
            gpio,
            fan,
            safety: SafetyPolicy::new(SafetyConfig::firmware()),
            applied_outputs: None,
            watchdog,
        }
    }

    fn now_ms() -> u64 {
        Instant::now().as_millis()
    }

    fn service_safety(&mut self) {
        let now_ms = Self::now_ms();
        let trip_input_asserted = self.gpio.asic_trip.is_high();
        self.safety.tick(now_ms, trip_input_asserted);
        self.apply_safety_outputs();
        // This is the only watchdog feed site. A stalled control/safety task
        // therefore resets the RP2040 back to its safe boot pin levels.
        self.watchdog.feed();
    }

    fn apply_safety_outputs(&mut self) {
        let outputs = self.safety.outputs();
        if self.applied_outputs == Some(outputs) {
            return;
        }

        let forward_asic_rx = bonanza_bridge_fw::uart_timing::asic_rx_forwarding_allowed(outputs.five_volt_enabled, outputs.asic_reset_asserted);
        if !forward_asic_rx {
            set_buffered_rx_forwarding_enabled(false);
        }
        let intent = outputs.board_control_intent();
        self.gpio.v5_en.set_level(intent.five_volt_enable_high.into());
        self.gpio.asic_rst.set_level(intent.asic_reset_n_high.into());
        fan::apply_percent(&mut self.fan.pwm, intent.fan_percent);
        if forward_asic_rx {
            set_buffered_rx_forwarding_enabled(true);
        }
        self.applied_outputs = Some(outputs);
    }

    fn safety_status_payload(&self) -> Vec<u8, 256> {
        let encoded = self.safety.status(Self::now_ms()).encode();
        Vec::from_slice(encoded.as_slice()).unwrap()
    }

    fn safety_arm(&mut self) -> Result<(), SafetyError> {
        self.service_safety();
        let result = self.safety.arm(Self::now_ms());
        self.apply_safety_outputs();
        result
    }

    fn safety_heartbeat(&mut self) -> Result<(), SafetyError> {
        self.service_safety();
        let result = self.safety.heartbeat(Self::now_ms());
        self.apply_safety_outputs();
        result
    }

    fn safety_clear_fault(&mut self) -> Result<(), SafetyError> {
        self.service_safety();
        let result = self.safety.clear_fault(Self::now_ms());
        self.apply_safety_outputs();
        result
    }

    fn safety_disarm(&mut self) {
        self.service_safety();
        self.safety.disarm();
        self.apply_safety_outputs();
    }

    fn safety_request_five_volt_enabled(&mut self, enabled: bool) -> Result<(), SafetyError> {
        self.service_safety();
        let result = self.safety.request_five_volt_enabled(enabled, Self::now_ms());
        self.apply_safety_outputs();
        result
    }

    fn safety_request_asic_reset_asserted(&mut self, asserted: bool) -> Result<(), SafetyError> {
        self.service_safety();
        let result = self.safety.request_asic_reset_asserted(asserted, Self::now_ms());
        self.apply_safety_outputs();
        result
    }

    fn safety_request_fan_percent(&mut self, percent: u8) -> Result<(), SafetyError> {
        self.service_safety();
        let result = self.safety.request_fan_percent(percent, Self::now_ms());
        self.apply_safety_outputs();
        result
    }

    async fn handle_command(&mut self, cmd: Command) -> Vec<u8, 260> {
        let res = match cmd.inner {
            CommandInner::System(cmd) => cmd.handle(self).await,
            CommandInner::Gpio(cmd) => cmd.handle(self).await,
            CommandInner::Fan(cmd) => cmd.handle(self).await,
        };

        match res {
            Ok(payload) => {
                let mut buf = Vec::<u8, 260>::new();
                buf.extend_from_slice(&[0x00, 0x00, cmd.id]).unwrap();
                buf.extend_from_slice(&payload).unwrap();
                let len = (buf.len() as u16).to_le_bytes();
                buf[0..2].clone_from_slice(&len);
                buf
            }
            Err(err) => err.encode(cmd.id),
        }
    }
}

#[embassy_executor::task]
pub async fn uart_task(mut uart: BufferedUart<'static, UART0>, gpio: gpio::Pins<'static>, fan: fan::Pins<'static>, watchdog: Watchdog) -> ! {
    let mut controller = Controller::new(gpio, fan, watchdog);
    let mut frame_buf = [0u8; 4098];
    let mut frame_len = 0usize;

    info!("Control: UART0 ready");
    controller.service_safety();

    loop {
        controller.service_safety();
        match read_packet(&mut uart, &mut frame_buf, &mut frame_len).await {
            Ok(Some(ReadPacketResult::Command(cmd))) => {
                let response = controller.handle_command(cmd).await;
                write_response(&mut controller, &mut uart, &response).await;
            }
            Ok(Some(ReadPacketResult::Error { id, error })) => {
                let response = error.encode(id);
                write_response(&mut controller, &mut uart, &response).await;
            }
            Ok(None) => {}
            Err(err) => {
                warn!("Control UART read error: {}", err);
            }
        }
    }
}

async fn write_response(controller: &mut Controller, uart: &mut BufferedUart<'static, UART0>, response: &[u8]) {
    match embassy_time::with_timeout(Duration::from_millis(CONTROL_WRITE_TIMEOUT_MS), write_all(uart, response)).await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => warn!("Control UART write error: {}", err),
        Err(TimeoutError) => warn!("Control UART write timeout"),
    }
    controller.service_safety();
}

async fn read_packet(uart: &mut BufferedUart<'static, UART0>, buf: &mut [u8; 4098], num_read: &mut usize) -> Result<Option<ReadPacketResult>, UartError> {
    if let Some(result) = try_extract_packet(buf, num_read) {
        return Ok(Some(result));
    }

    match embassy_time::with_timeout(Duration::from_millis(4), uart.read(&mut buf[*num_read..])).await {
        Ok(Ok(0)) => Ok(None),
        Ok(Ok(n)) => {
            *num_read += n;
            Ok(try_extract_packet(buf, num_read))
        }
        Ok(Err(err)) => Err(err),
        Err(TimeoutError) => {
            if *num_read == 0 {
                return Ok(None);
            }

            let id = if *num_read >= 3 { buf[2] } else { 0xff };
            *num_read = 0;

            Ok(Some(ReadPacketResult::Error { id, error: CommandError::Timeout }))
        }
    }
}

fn try_extract_packet(buf: &mut [u8; 4098], num_read: &mut usize) -> Option<ReadPacketResult> {
    if *num_read < 2 {
        return None;
    }

    let packet_len = u16::from_le_bytes(buf[0..2].try_into().unwrap()) as usize;
    if !(6..=buf.len()).contains(&packet_len) {
        let id = if *num_read >= 3 { buf[2] } else { 0xff };
        *num_read = 0;
        return Some(ReadPacketResult::Error { id, error: CommandError::Invalid });
    }

    if *num_read < packet_len {
        return None;
    }

    let id = buf[2];
    let result = match Command::from_bytes(&buf[2..packet_len]) {
        Ok(cmd) => ReadPacketResult::Command(cmd),
        Err(error) => ReadPacketResult::Error { id, error },
    };

    let excess = *num_read - packet_len;
    if excess > 0 {
        buf.copy_within(packet_len..packet_len + excess, 0);
    }
    *num_read = excess;

    Some(result)
}

async fn write_all<W>(writer: &mut W, mut buf: &[u8]) -> Result<(), W::Error>
where
    W: Write,
{
    while !buf.is_empty() {
        let written = writer.write(buf).await?;
        if written == 0 {
            continue;
        }
        buf = &buf[written..];
    }

    writer.flush().await
}
