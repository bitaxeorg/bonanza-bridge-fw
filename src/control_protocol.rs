//! Read-only 1004x bridge diagnostics. Board control belongs to the ESP32.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    Info,
    RxStats,
}

pub fn decode(packet: &[u8]) -> Option<(u8, Command)> {
    match packet {
        [6, 0, id, 0, 0, 1] => Some((*id, Command::Info)),
        [6, 0, id, 0, 0, 2] => Some((*id, Command::RxStats)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_exact_read_only_diagnostic_packets_are_accepted() {
        assert_eq!(decode(&[6, 0, 42, 0, 0, 1]), Some((42, Command::Info)));
        assert_eq!(decode(&[6, 0, 43, 0, 0, 2]), Some((43, Command::RxStats)));
        for packet in [
            &[6, 0, 42, 0, 6, 1][..],     // Old GPIO command.
            &[7, 0, 42, 0, 9, 0x10, 100], // Old fan command.
            &[6, 0, 42, 0, 0, 0x11],      // Old lease arm.
            &[6, 0, 42, 1, 0, 1],         // Invalid bus.
            &[7, 0, 42, 0, 0, 1, 0],      // Unexpected payload.
            &[6, 0, 42, 0, 0],            // Truncated request.
        ] {
            assert_eq!(decode(packet), None);
        }
    }
}
