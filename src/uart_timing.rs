//! Pure timing helpers for the RP2040 PIO UART.

pub const PIO_UART_CYCLES_PER_BIT: u32 = 8;
pub const RX_FIRST_DATA_SAMPLE_CYCLE: u32 = 12;
pub const PIO_RX_FIFO_BYTES: usize = 8;
pub const ASIC_TELEMETRY_FRAME_BYTES: usize = 10;
pub const ASIC_RX_WIRE_BITS_PER_BYTE: u8 = 11;
pub const ESP_TX_WIRE_BITS_PER_BYTE: u8 = 10;
pub const ASIC_RX_DMA_RING_WORDS: usize = 1024;
pub const ASIC_RX_DMA_TRANSFER_COUNT: u32 = 0x7fff_ffff;
pub const ASIC_RX_DESIGN_WORDS_PER_SECOND: u32 = 16_000;
pub const ESP_DATA_BAUD_RATE: u32 = 2_000_000;
pub const QUALIFICATION_SOAK_SECONDS: u32 = 24 * 60 * 60;

/// ASIC output is meaningful only while its I/O rail is enabled and reset is
/// released. Keeping the PIO state machine active in any other state can turn
/// an unpowered low RX line into an endless stream of synthetic zero bytes.
pub const fn asic_rx_forwarding_allowed(five_volt_enabled: bool, asic_reset_asserted: bool) -> bool {
    five_volt_enabled && !asic_reset_asserted
}

/// Whether a complete ASIC response can be buffered without software draining
/// the PIO FIFO while the frame is arriving.
pub const fn rx_fifo_holds_complete_frame(frame_bytes: usize) -> bool {
    frame_bytes <= PIO_RX_FIFO_BYTES
}

/// Resolve the readable DMA-ring window from monotonically increasing transfer
/// counts. If the producer lapped the consumer, retain the newest ring and
/// report exactly how many older words were overwritten.
pub const fn dma_ring_window(produced: u32, consumed: u32, capacity: usize) -> (u32, usize, u32) {
    let available = produced.wrapping_sub(consumed) as usize;
    if available > capacity {
        let dropped = available - capacity;
        (produced.wrapping_sub(capacity as u32), capacity, dropped as u32)
    } else {
        (consumed, available, 0)
    }
}

/// Return the RP2040 16.8 PIO clock divider bits for a UART baud rate.
pub fn clock_divider_bits(system_clock_hz: u32, baudrate: u32) -> Option<u32> {
    let divisor = (baudrate as u64).checked_mul(PIO_UART_CYCLES_PER_BIT as u64)?;
    if system_clock_hz == 0 || divisor == 0 {
        return None;
    }

    let bits = (system_clock_hz as u64).checked_mul(256)?.checked_div(divisor)?;
    if bits == 0 || bits > u32::MAX as u64 {
        return None;
    }
    Some(bits as u32)
}

/// PIO-cycle position at which an RX data bit is sampled, relative to the
/// start-bit detection instruction. The returned position is the center of
/// each nominal data bit for the eight-cycle receiver program.
pub const fn rx_data_sample_cycle(bit_index: u8) -> Option<u32> {
    if bit_index >= 9 {
        return None;
    }
    Some(RX_FIRST_DATA_SAMPLE_CYCLE + bit_index as u32 * PIO_UART_CYCLES_PER_BIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_mbaud_divider_is_exact_at_125_mhz() {
        // 3.125 in the RP2040's 16.8 fixed-point representation.
        assert_eq!(clock_divider_bits(125_000_000, 5_000_000), Some(3 * 256 + 32));
    }

    #[test]
    fn rx_samples_all_nine_bits_at_consistent_eight_cycle_intervals() {
        assert_eq!(rx_data_sample_cycle(0), Some(12));
        assert_eq!(rx_data_sample_cycle(7), Some(68));
        assert_eq!(rx_data_sample_cycle(8), Some(76));
        assert_eq!(rx_data_sample_cycle(9), None);
    }

    #[test]
    fn rx_first_payload_sample_is_centered_at_one_and_a_half_bits() {
        assert_eq!(RX_FIRST_DATA_SAMPLE_CYCLE * 2, PIO_UART_CYCLES_PER_BIT * 3);
    }

    #[test]
    fn invalid_clock_inputs_fail_closed() {
        assert_eq!(clock_divider_bits(0, 5_000_000), None);
        assert_eq!(clock_divider_bits(125_000_000, 0), None);
        assert_eq!(clock_divider_bits(1, u32::MAX), None);
    }

    #[test]
    fn telemetry_requires_concurrent_fifo_draining() {
        assert!(!rx_fifo_holds_complete_frame(ASIC_TELEMETRY_FRAME_BYTES));
        assert!(rx_fifo_holds_complete_frame(PIO_RX_FIFO_BYTES));
    }

    #[test]
    fn raw_rx_link_covers_the_measured_design_rate() {
        let required_baud = ASIC_RX_DESIGN_WORDS_PER_SECOND * ESP_TX_WIRE_BITS_PER_BYTE as u32;
        assert_eq!(required_baud, 160_000);
        assert!(required_baud * 12 < ESP_DATA_BAUD_RATE);
    }

    #[test]
    fn dma_ring_holds_many_complete_tdm_frames() {
        assert_eq!(ASIC_RX_DMA_RING_WORDS, 1024);
        assert_eq!(ASIC_RX_DMA_RING_WORDS * size_of::<u32>(), 1 << 12);
        assert!(ASIC_RX_DMA_RING_WORDS >= 100 * ASIC_TELEMETRY_FRAME_BYTES);
    }

    #[test]
    fn dma_ring_window_reports_exact_overwrite_loss() {
        assert_eq!(dma_ring_window(25, 20, 16), (20, 5, 0));
        assert_eq!(dma_ring_window(50, 20, 16), (34, 16, 14));
        assert_eq!(dma_ring_window(3, u32::MAX - 2, 16), (u32::MAX - 2, 6, 0));
    }

    #[test]
    fn dma_transfer_counter_covers_the_full_qualification_soak() {
        assert!((ASIC_RX_DESIGN_WORDS_PER_SECOND as u64) * (QUALIFICATION_SOAK_SECONDS as u64) < ASIC_RX_DMA_TRANSFER_COUNT as u64);
    }

    #[test]
    fn asic_rx_is_gated_by_io_power_and_reset() {
        assert!(!asic_rx_forwarding_allowed(false, true));
        assert!(!asic_rx_forwarding_allowed(false, false));
        assert!(!asic_rx_forwarding_allowed(true, true));
        assert!(asic_rx_forwarding_allowed(true, false));
    }
}
