//! Pure ESP-side byte-pair decoding for the ASIC 9-bit UART.

#[derive(Default)]
pub struct NineBitPairDecoder {
    pending_low_byte: Option<u8>,
}

/// Fixed-capacity interrupt batch that cannot index past its backing array
/// when a hardware FIFO refills while it is being drained.
pub struct ByteBatch<const N: usize> {
    bytes: [u8; N],
    length: usize,
}

impl<const N: usize> ByteBatch<N> {
    pub const fn new() -> Self {
        assert!(N != 0);
        Self { bytes: [0; N], length: 0 }
    }

    pub fn try_push(&mut self, byte: u8) -> Result<(), u8> {
        if self.length == N {
            return Err(byte);
        }
        self.bytes[self.length] = byte;
        self.length += 1;
        Ok(())
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.length]
    }

    pub const fn is_full(&self) -> bool {
        self.length == N
    }

    pub fn clear(&mut self) {
        self.length = 0;
    }
}

impl<const N: usize> Default for ByteBatch<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Right-shift autopush at nine bits places the complete UART word in bits
/// 31:23. The first serial bit remains bit 0 after extraction.
pub const fn asic_rx_word_to_u16(word: u32) -> u16 {
    ((word >> 23) & 0x1ff) as u16
}

/// Extract only the eight payload bits forwarded by the BIRDS-compatible RX
/// path. PIO still samples all nine data bits before requiring the stop bit.
pub const fn asic_rx_word_to_u8(word: u32) -> u8 {
    asic_rx_word_to_u16(word) as u8
}

impl NineBitPairDecoder {
    pub const fn new() -> Self {
        Self { pending_low_byte: None }
    }

    /// Decode `[low byte, bit 8]` pairs into 9-bit words. A low byte split
    /// across input chunks is retained for the next call. The bit-8 byte is
    /// constrained by the ESP/bridge wire contract to exactly zero or one.
    /// If an ESP reset or dropped UART byte leaves the stream out of phase,
    /// an invalid bit-8 byte becomes the next low-byte candidate so the next
    /// valid bit-8 byte restores alignment without resetting the bridge.
    pub fn decode(&mut self, input: &[u8], output: &mut [u16]) -> usize {
        let mut input_index = 0;
        let mut output_length = 0;

        if let Some(low_byte) = self.pending_low_byte.take() {
            if input.is_empty() || output.is_empty() {
                self.pending_low_byte = Some(low_byte);
                return 0;
            }
            if input[0] <= 1 {
                output[output_length] = low_byte as u16 | ((input[0] as u16) << 8);
                output_length += 1;
            } else {
                self.pending_low_byte = Some(input[0]);
            }
            input_index = 1;
        }

        while input_index < input.len() && output_length < output.len() {
            let low_byte = match self.pending_low_byte.take() {
                Some(low_byte) => low_byte,
                None => {
                    let low_byte = input[input_index];
                    input_index += 1;
                    if input_index == input.len() {
                        self.pending_low_byte = Some(low_byte);
                        break;
                    }
                    low_byte
                }
            };

            let bit_8 = input[input_index];
            input_index += 1;
            if bit_8 <= 1 {
                output[output_length] = low_byte as u16 | ((bit_8 as u16) << 8);
                output_length += 1;
            } else {
                self.pending_low_byte = Some(bit_8);
            }
        }

        if input_index < input.len() {
            // The bridge's input and output buffers are sized so output space
            // cannot be exhausted before all complete input pairs are decoded.
            if input_index + 1 == input.len() {
                self.pending_low_byte = Some(input[input_index]);
            }
        }

        output_length
    }

    pub const fn has_pending_byte(&self) -> bool {
        self.pending_low_byte.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_batch_reports_full_without_overwriting_or_panicking() {
        let mut batch = ByteBatch::<2>::new();

        assert_eq!(batch.try_push(0x11), Ok(()));
        assert_eq!(batch.try_push(0x22), Ok(()));
        assert!(batch.is_full());
        assert_eq!(batch.try_push(0x33), Err(0x33));
        assert_eq!(batch.as_slice(), &[0x11, 0x22]);
    }

    #[test]
    fn byte_batch_can_be_reused_after_a_full_flush() {
        let mut batch = ByteBatch::<2>::new();

        batch.try_push(0x11).unwrap();
        batch.try_push(0x22).unwrap();
        batch.clear();
        assert!(!batch.is_full());
        assert_eq!(batch.as_slice(), &[]);
        assert_eq!(batch.try_push(0x33), Ok(()));
        assert_eq!(batch.as_slice(), &[0x33]);
    }

    #[test]
    fn raw_rx_forwarding_drops_only_the_ninth_bit() {
        assert_eq!(asic_rx_word_to_u16(0x1a5 << 23), 0x1a5);
        assert_eq!(asic_rx_word_to_u16(0x082 << 23), 0x082);
        assert_eq!(asic_rx_word_to_u8(0x1a5 << 23), 0xa5);
        assert_eq!(asic_rx_word_to_u8(0x0a5 << 23), 0xa5);
    }

    #[test]
    fn complete_pairs_decode_to_nine_bit_words() {
        let mut decoder = NineBitPairDecoder::new();
        let mut words = [0u16; 3];
        let count = decoder.decode(&[0x55, 1, 0xaa, 0, 0xff, 1], &mut words);

        assert_eq!(count, 3);
        assert_eq!(words, [0x155, 0x0aa, 0x1ff]);
        assert!(!decoder.has_pending_byte());
    }

    #[test]
    fn stale_pending_byte_resynchronizes_on_the_next_complete_pair() {
        let mut decoder = NineBitPairDecoder::new();
        let mut words = [0u16; 2];

        assert_eq!(decoder.decode(&[0x7e], &mut words), 0);
        assert!(decoder.has_pending_byte());

        let count = decoder.decode(&[0xfa, 1, 0xf0, 0], &mut words);
        assert_eq!(count, 2);
        assert_eq!(words, [0x1fa, 0x0f0]);
        assert!(!decoder.has_pending_byte());
    }

    #[test]
    fn dropped_byte_resynchronizes_without_masking_invalid_bit_byte() {
        let mut decoder = NineBitPairDecoder::new();
        let mut words = [0u16; 3];

        // The bit byte for 0x155 was lost. 0xaa cannot be a bit-8 byte, so it
        // becomes the low byte for the next valid pair.
        let count = decoder.decode(&[0x55, 0xaa, 0, 0xff, 1], &mut words);
        assert_eq!(count, 2);
        assert_eq!(&words[..count], &[0x0aa, 0x1ff]);
        assert!(!decoder.has_pending_byte());
    }

    #[test]
    fn split_pairs_are_preserved_across_reads() {
        let mut decoder = NineBitPairDecoder::new();
        let mut words = [0u16; 2];

        assert_eq!(decoder.decode(&[0x12], &mut words), 0);
        assert!(decoder.has_pending_byte());
        assert_eq!(decoder.decode(&[1, 0x34, 0], &mut words), 2);
        assert_eq!(words, [0x112, 0x034]);
        assert!(!decoder.has_pending_byte());
    }

    #[test]
    fn empty_reads_do_not_lose_pending_data() {
        let mut decoder = NineBitPairDecoder::new();
        let mut words = [0u16; 1];

        assert_eq!(decoder.decode(&[0x5a], &mut words), 0);
        assert_eq!(decoder.decode(&[], &mut words), 0);
        assert!(decoder.has_pending_byte());
        assert_eq!(decoder.decode(&[1], &mut words), 1);
        assert_eq!(words[0], 0x15a);
    }
}
