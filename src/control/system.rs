use bonanza_bridge_fw::info;
use heapless::Vec;

use super::{CommandError, Controller, ControllerCommand};

#[derive(defmt::Format)]
pub enum Command {
    GetInfo,
}

impl Command {
    pub fn from_bytes(buf: &[u8]) -> Result<Self, CommandError> {
        match buf {
            [0x01] => Ok(Self::GetInfo),
            _ => Err(CommandError::Invalid),
        }
    }
}

impl ControllerCommand for Command {
    async fn handle(&self, _controller: &mut Controller) -> Result<Vec<u8, 256>, CommandError> {
        match self {
            Command::GetInfo => info::firmware_info().map_err(|_| CommandError::Invalid),
        }
    }
}
