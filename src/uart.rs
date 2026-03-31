use defmt::warn;

use embassy_futures::select::{select, Either};
use embassy_rp::{
    peripherals::{PIO1, UART1},
    uart::{BufferedUart, BufferedUartRx, BufferedUartTx, Error as SerialError},
};
use embedded_io_async::{Read, Write};

use crate::pio_uart::PioUart;

#[embassy_executor::task]
pub async fn uart_task(serial: BufferedUart<'static, UART1>, mut uart: PioUart<'static, PIO1, 0, 1>) -> ! {
    let (mut serial_tx, mut serial_rx) = serial.split();

    loop {
        if let Err(err) = pipe_uart(&mut serial_tx, &mut serial_rx, &mut uart).await {
            warn!("Data UART bridge error: {}", err);
        }
    }
}

/// Handle ESP32 UART1 <-> ASIC 9-bit UART forwarding.
///
/// 9-bit serial data is encoded as byte pairs on UART1:
/// - First byte: lower 8 bits of the 9-bit word
/// - Second byte: bit 8 (0 or 1)
pub async fn pipe_uart(serial_tx: &mut BufferedUartTx<'static, UART1>, serial_rx: &mut BufferedUartRx<'static, UART1>, uart: &mut PioUart<'static, PIO1, 0, 1>) -> Result<(), SerialError> {
    let mut serial_buf = [0u8; 64];
    let mut uart_buf = [0u8; 64];
    let mut pending_byte: Option<u8> = None;

    loop {
        match select(serial_rx.read(&mut serial_buf), uart.read_u16()).await {
            Either::First(result) => {
                let n = result?;
                if n == 0 {
                    continue;
                }

                let data = &serial_buf[..n];
                let mut i = 0;

                if let Some(low_byte) = pending_byte.take() {
                    if i < n {
                        let bit8 = data[i] & 0x01;
                        let word = (low_byte as u16) | ((bit8 as u16) << 8);
                        uart.write_u16(word).await;
                        i += 1;
                    } else {
                        pending_byte = Some(low_byte);
                    }
                }

                while i + 1 < n {
                    let low_byte = data[i];
                    let bit8 = data[i + 1] & 0x01;
                    let word = (low_byte as u16) | ((bit8 as u16) << 8);
                    uart.write_u16(word).await;
                    i += 2;
                }

                if i < n {
                    pending_byte = Some(data[i]);
                }
            }
            Either::Second(word) => {
                let mut count = 0;
                uart_buf[count] = (word & 0xFF) as u8;
                uart_buf[count + 1] = ((word >> 8) & 0x01) as u8;
                count += 2;

                while count + 1 < uart_buf.len() {
                    if let Some(word) = uart.try_read() {
                        uart_buf[count] = (word & 0xFF) as u8;
                        uart_buf[count + 1] = ((word >> 8) & 0x01) as u8;
                        count += 2;
                    } else {
                        break;
                    }
                }

                write_all(serial_tx, &uart_buf[..count]).await?;
            }
        }
    }
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
