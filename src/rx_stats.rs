use heapless::Vec;

pub const SCHEMA_VERSION: u8 = 1;
pub const PAYLOAD_LENGTH: usize = 9;

/// Encode cumulative ASIC receive loss counters for the control protocol.
pub fn encode(pio_fifo_overflows: u32, software_ring_overflows: u32) -> Vec<u8, PAYLOAD_LENGTH> {
    let mut payload = Vec::new();
    payload.push(SCHEMA_VERSION).unwrap();
    payload.extend_from_slice(&pio_fifo_overflows.to_le_bytes()).unwrap();
    payload.extend_from_slice(&software_ring_overflows.to_le_bytes()).unwrap();
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rx_stats_payload_is_fixed_width_and_little_endian() {
        let payload = encode(0x1234_5678, 0x90ab_cdef);

        assert_eq!(payload.as_slice(), &[1, 0x78, 0x56, 0x34, 0x12, 0xef, 0xcd, 0xab, 0x90]);
    }
}
