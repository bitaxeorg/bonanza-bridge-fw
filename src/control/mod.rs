use defmt::{info, warn};

use embassy_rp::{
    peripherals::UART0,
    uart::{BufferedUart, Error as UartError},
};
use embassy_time::{Duration, TimeoutError};
use embedded_io_async::{Read, Write};
use heapless::Vec;

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
    Gpio(gpio::Command),
    Fan(fan::Command),
}

impl Command {
    fn from_bytes(buf: &[u8]) -> Result<Self, CommandError> {
        let id = buf[0];
        match buf[2] {
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
}

pub trait ControllerCommand {
    async fn handle(&self, controller: &mut Controller) -> Result<Vec<u8, 256>, CommandError>;
}

impl Controller {
    async fn handle_command(&mut self, cmd: Command) -> Vec<u8, 260> {
        let res = match cmd.inner {
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
pub async fn uart_task(mut uart: BufferedUart<'static, UART0>, gpio: gpio::Pins<'static>, fan: fan::Pins<'static>) -> ! {
    let mut controller = Controller { gpio, fan };
    let mut frame_buf = [0u8; 4098];
    let mut frame_len = 0usize;

    info!("Control: UART0 ready");

    loop {
        match read_packet(&mut uart, &mut frame_buf, &mut frame_len).await {
            Ok(Some(ReadPacketResult::Command(cmd))) => {
                let response = controller.handle_command(cmd).await;
                if let Err(err) = write_all(&mut uart, &response).await {
                    warn!("Control UART write error: {}", err);
                }
            }
            Ok(Some(ReadPacketResult::Error { id, error })) => {
                let response = error.encode(id);
                if let Err(err) = write_all(&mut uart, &response).await {
                    warn!("Control UART write error: {}", err);
                }
            }
            Ok(None) => {}
            Err(err) => {
                warn!("Control UART read error: {}", err);
            }
        }
    }
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
