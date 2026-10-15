//! Read-only UART0 diagnostics. No GPIO, fan, lease or power control on 1004x.
use bonanza_bridge_fw::{
    control_protocol::{decode, Command},
    info, rx_stats,
};
use defmt::warn;
use embassy_rp::{peripherals::UART0, uart::BufferedUart};
use embassy_time::{with_timeout, Duration};
use embedded_io_async::{Read, Write};
use heapless::Vec;

use crate::pio_uart::buffered_rx_overflows;

#[embassy_executor::task]
pub async fn uart_task(mut uart: BufferedUart<'static, UART0>) -> ! {
    // Valid requests are six bytes. Extra room lets us reject and consume
    // legacy payload-bearing requests without losing subsequent packet IDs.
    let mut frame = [0u8; 256];
    let mut used = 0;
    loop {
        while used >= 2 {
            let length = u16::from_le_bytes([frame[0], frame[1]]) as usize;
            if !(6..=frame.len()).contains(&length) {
                let id = frame.get(2).filter(|_| used >= 3).copied().unwrap_or(0xff);
                reply(&mut uart, id, &[0x11]).await;
                used = 0;
                break;
            }
            if used < length {
                break;
            }
            let id = frame[2];
            let payload = match decode(&frame[..length]) {
                Some((_, Command::Info)) => info::firmware_info().unwrap(),
                Some((_, Command::RxStats)) => {
                    let (fifo, ring) = buffered_rx_overflows();
                    Vec::from_slice(rx_stats::encode(fifo, ring).as_slice()).unwrap()
                }
                None => Vec::from_slice(&[0x11]).unwrap(),
            };
            reply(&mut uart, id, &payload).await;
            frame.copy_within(length..used, 0);
            used -= length;
        }
        match with_timeout(Duration::from_millis(4), uart.read(&mut frame[used..])).await {
            Ok(Ok(count)) => used += count,
            Ok(Err(err)) => {
                warn!("Control UART read error: {}", err);
                used = 0;
            }
            Err(_) if used != 0 => {
                let id = if used >= 3 { frame[2] } else { 0xff };
                reply(&mut uart, id, &[0x10]).await;
                used = 0;
            }
            Err(_) => {}
        }
    }
}

async fn reply(uart: &mut BufferedUart<'static, UART0>, id: u8, payload: &[u8]) {
    let mut frame = Vec::<u8, 260>::new();
    frame.extend_from_slice(&((payload.len() + 3) as u16).to_le_bytes()).unwrap();
    frame.push(id).unwrap();
    frame.extend_from_slice(payload).unwrap();
    let write = async {
        let mut remaining = frame.as_slice();
        while !remaining.is_empty() {
            let count = uart.write(remaining).await?;
            remaining = &remaining[count..];
        }
        Ok::<(), embassy_rp::uart::Error>(())
    };
    if !matches!(with_timeout(Duration::from_millis(100), write).await, Ok(Ok(()))) {
        warn!("Control UART reply failed or timed out");
    }
}
