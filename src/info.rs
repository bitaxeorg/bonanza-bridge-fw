use heapless::Vec;

pub const INFO_SCHEMA_VERSION: u8 = 1;
pub const PROTOCOL_MAJOR: u8 = 1;
/// Protocol 1.0 forwards raw ASIC RX bytes and exposes receive-loss counters
/// through the system control page.
pub const PROTOCOL_MINOR: u8 = 0;
pub const VERSION_MAX_LENGTH: usize = 63;
pub const FIRMWARE_VERSION: &str = env!("BRIDGE_FIRMWARE_VERSION");

#[derive(Debug, Eq, PartialEq)]
pub enum EncodeError {
    EmptyVersion,
    VersionTooLong,
    VersionNotPrintableAscii,
}

pub fn encode(version: &str) -> Result<Vec<u8, 256>, EncodeError> {
    if version.is_empty() {
        return Err(EncodeError::EmptyVersion);
    }
    if version.len() > VERSION_MAX_LENGTH {
        return Err(EncodeError::VersionTooLong);
    }
    if !version.bytes().all(|byte| (0x20..=0x7e).contains(&byte)) {
        return Err(EncodeError::VersionNotPrintableAscii);
    }

    let mut payload = Vec::new();
    payload.extend_from_slice(&[INFO_SCHEMA_VERSION, PROTOCOL_MAJOR, PROTOCOL_MINOR, version.len() as u8]).unwrap();
    payload.extend_from_slice(version.as_bytes()).unwrap();
    Ok(payload)
}

pub fn firmware_info() -> Result<Vec<u8, 256>, EncodeError> {
    encode(FIRMWARE_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firmware_info_matches_the_wire_schema() {
        let payload = firmware_info().unwrap();

        assert_eq!(payload[0], INFO_SCHEMA_VERSION);
        assert_eq!(payload[1], PROTOCOL_MAJOR);
        assert_eq!(payload[2], PROTOCOL_MINOR);
        assert_eq!(payload[3] as usize, FIRMWARE_VERSION.len());
        assert_eq!(&payload[4..], FIRMWARE_VERSION.as_bytes());
    }

    #[test]
    fn accepts_the_maximum_version_length() {
        let version = "v".repeat(VERSION_MAX_LENGTH);
        let payload = encode(&version).unwrap();

        assert_eq!(payload.len(), VERSION_MAX_LENGTH + 4);
        assert_eq!(payload[3] as usize, VERSION_MAX_LENGTH);
    }

    #[test]
    fn rejects_versions_the_esp_miner_cannot_decode() {
        assert_eq!(encode(""), Err(EncodeError::EmptyVersion));
        assert_eq!(encode(&"v".repeat(VERSION_MAX_LENGTH + 1)), Err(EncodeError::VersionTooLong));
        assert_eq!(encode("0.0.1\n"), Err(EncodeError::VersionNotPrintableAscii));
        assert_eq!(encode("0.0.1-é"), Err(EncodeError::VersionNotPrintableAscii));
    }
}
