use crate::info::{FIRMWARE_VERSION, PROTOCOL_MAJOR, PROTOCOL_MINOR, VERSION_MAX_LENGTH};

pub const MANIFEST_MAGIC: &[u8; 16] = b"BZM-BRIDGE-FW\0\0\0";
pub const MANIFEST_SCHEMA_VERSION: u8 = 1;
pub const MANIFEST_SIZE: usize = 96;
pub const MANIFEST_CRC_OFFSET: usize = MANIFEST_SIZE - 4;
pub const TARGET_BOARD_VERSION: u16 = 1002;
pub const FIRMWARE_KIND_BRIDGE: u8 = 1;
pub const VERSION_OFFSET: usize = 24;
pub const VERSION_CAPACITY: usize = 64;

const fn crc32(bytes: &[u8], length: usize) -> u32 {
    let mut crc = 0xffff_ffffu32;
    let mut index = 0;
    while index < length {
        crc ^= bytes[index] as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = (crc >> 1) ^ (0xedb8_8320u32 & (0u32.wrapping_sub(crc & 1)));
            bit += 1;
        }
        index += 1;
    }
    !crc
}

pub const fn encode(version: &str) -> [u8; MANIFEST_SIZE] {
    let version_bytes = version.as_bytes();
    assert!(!version_bytes.is_empty());
    assert!(version_bytes.len() <= VERSION_MAX_LENGTH);

    let mut manifest = [0u8; MANIFEST_SIZE];
    let mut index = 0;
    while index < MANIFEST_MAGIC.len() {
        manifest[index] = MANIFEST_MAGIC[index];
        index += 1;
    }

    manifest[16] = MANIFEST_SCHEMA_VERSION;
    manifest[17] = MANIFEST_SIZE as u8;
    manifest[18] = TARGET_BOARD_VERSION as u8;
    manifest[19] = (TARGET_BOARD_VERSION >> 8) as u8;
    manifest[20] = FIRMWARE_KIND_BRIDGE;
    manifest[21] = PROTOCOL_MAJOR;
    manifest[22] = PROTOCOL_MINOR;
    manifest[23] = version_bytes.len() as u8;

    index = 0;
    while index < version_bytes.len() {
        let byte = version_bytes[index];
        assert!(byte >= 0x20 && byte <= 0x7e);
        manifest[VERSION_OFFSET + index] = byte;
        index += 1;
    }

    let crc = crc32(&manifest, MANIFEST_CRC_OFFSET);
    manifest[MANIFEST_CRC_OFFSET] = crc as u8;
    manifest[MANIFEST_CRC_OFFSET + 1] = (crc >> 8) as u8;
    manifest[MANIFEST_CRC_OFFSET + 2] = (crc >> 16) as u8;
    manifest[MANIFEST_CRC_OFFSET + 3] = (crc >> 24) as u8;
    manifest
}

pub const IMAGE_MANIFEST: [u8; MANIFEST_SIZE] = encode(FIRMWARE_VERSION);

#[cfg(test)]
mod tests {
    use super::*;

    fn read_le32(bytes: &[u8]) -> u32 {
        u32::from_le_bytes(bytes.try_into().unwrap())
    }

    #[test]
    fn firmware_manifest_identifies_the_bridge_image() {
        assert_eq!(&IMAGE_MANIFEST[..MANIFEST_MAGIC.len()], MANIFEST_MAGIC);
        assert_eq!(IMAGE_MANIFEST[16], MANIFEST_SCHEMA_VERSION);
        assert_eq!(IMAGE_MANIFEST[17] as usize, MANIFEST_SIZE);
        assert_eq!(u16::from_le_bytes([IMAGE_MANIFEST[18], IMAGE_MANIFEST[19]]), TARGET_BOARD_VERSION);
        assert_eq!(IMAGE_MANIFEST[20], FIRMWARE_KIND_BRIDGE);
        assert_eq!(IMAGE_MANIFEST[21], PROTOCOL_MAJOR);
        assert_eq!(IMAGE_MANIFEST[22], PROTOCOL_MINOR);
        assert_eq!(IMAGE_MANIFEST[23] as usize, FIRMWARE_VERSION.len());
        assert_eq!(&IMAGE_MANIFEST[VERSION_OFFSET..VERSION_OFFSET + FIRMWARE_VERSION.len()], FIRMWARE_VERSION.as_bytes());
        assert!(IMAGE_MANIFEST[VERSION_OFFSET + FIRMWARE_VERSION.len()..MANIFEST_CRC_OFFSET].iter().all(|byte| *byte == 0));
        assert_eq!(read_le32(&IMAGE_MANIFEST[MANIFEST_CRC_OFFSET..]), crc32(&IMAGE_MANIFEST, MANIFEST_CRC_OFFSET));
    }

    #[test]
    fn manifest_encoding_is_deterministic_and_versioned() {
        let manifest = encode("1.2.3-test");

        assert_eq!(&manifest[..MANIFEST_MAGIC.len()], MANIFEST_MAGIC);
        assert_eq!(manifest[23], 10);
        assert_eq!(&manifest[VERSION_OFFSET..VERSION_OFFSET + 10], b"1.2.3-test");
        assert_eq!(read_le32(&manifest[MANIFEST_CRC_OFFSET..]), crc32(&manifest, MANIFEST_CRC_OFFSET));
    }
}
