use bonanza_bridge_fw::{info, rx_stats};
use heapless::Vec;

use crate::pio_uart::buffered_rx_overflows;

use super::{CommandError, Controller, ControllerCommand};

#[derive(defmt::Format)]
pub enum Command {
    GetInfo,
    GetRxStats,
}

impl Command {
    pub fn from_bytes(buf: &[u8]) -> Result<Self, CommandError> {
        match buf {
            [0x01] => Ok(Self::GetInfo),
            [0x02] => Ok(Self::GetRxStats),
            _ => Err(CommandError::Invalid),
        }
    }
}

impl ControllerCommand for Command {
    async fn handle(&self, _controller: &mut Controller) -> Result<Vec<u8, 256>, CommandError> {
        match self {
            Command::GetInfo => info::firmware_info().map_err(|_| CommandError::Invalid),
            Command::GetRxStats => {
                let (pio_fifo_overflows, software_ring_overflows) = buffered_rx_overflows();
                let encoded = rx_stats::encode(pio_fifo_overflows, software_ring_overflows);
                Ok(Vec::from_slice(encoded.as_slice()).unwrap())
            }
        }
    }
}
