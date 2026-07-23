use bonanza_bridge_fw::uart_codec::NineBitPairDecoder;
use defmt::warn;
use embassy_rp::{
    peripherals::{DMA_CH0, PIO1, UART1},
    uart::{BufferedUartRx, BufferedUartTx},
};
use embedded_io_async::{Read, Write};

use crate::pio_uart::{buffered_rx_overflows, receive_buffered_rx_chunk, PioUartRx, PioUartTx};

/// Forward ESP32 UART1 byte pairs to the ASIC 9-bit TX state machine.
///
/// This task is deliberately independent from ASIC RX forwarding. A burst of
/// ESP commands can therefore never prevent the PIO RX FIFO from being drained.
#[embassy_executor::task]
pub async fn esp_to_asic_task(mut serial_rx: BufferedUartRx<'static, UART1>, mut uart_tx: PioUartTx<'static, PIO1, 0>) -> ! {
    let mut serial_buf = [0u8; 64];
    let mut words = [0u16; 32];
    let mut decoder = NineBitPairDecoder::new();

    loop {
        match serial_rx.read(&mut serial_buf).await {
            Ok(0) => {}
            Ok(count) => {
                let word_count = decoder.decode(&serial_buf[..count], &mut words);
                for word in &words[..word_count] {
                    uart_tx.write_u16(*word).await;
                }
            }
            Err(err) => warn!("ESP data UART RX error: {}", err),
        }
    }
}

/// Forward BIRDS-compatible raw ASIC RX bytes into the ESP UART TX ring. PIO
/// still samples complete 9N1 words and requires the stop bit, but bit 8 is not
/// part of the ASIC response protocol consumed by the ESP. DMA draining keeps
/// scheduler or interrupt latency from filling the eight-word PIO FIFO.
#[embassy_executor::task]
pub async fn asic_to_esp_task(mut serial_tx: BufferedUartTx<'static, UART1>, mut asic_rx: PioUartRx<'static, PIO1, 1>, dma: DMA_CH0) -> ! {
    // Retain both peripheral guards for the task lifetime. The state machine
    // and channel are driven through PAC registers because Embassy's finite
    // DMA future does not expose the RP2040's address-ring mode.
    let _asic_rx = &mut asic_rx;
    let _dma = dma;
    let mut bytes = [0u8; 64];
    let mut reported_overflows = (0u32, 0u32);

    loop {
        let count = receive_buffered_rx_chunk(&mut bytes).await;
        let overflows = buffered_rx_overflows();

        if overflows != reported_overflows {
            warn!("ASIC RX overflow counters: PIO FIFO={}, software ring={}", overflows.0, overflows.1);
            reported_overflows = overflows;
        }

        if let Err(err) = write_all_buffered(&mut serial_tx, &bytes[..count]).await {
            warn!("ESP data UART TX error: {}", err);
        }
    }
}

async fn write_all_buffered<W>(writer: &mut W, mut buf: &[u8]) -> Result<(), W::Error>
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
    Ok(())
}
