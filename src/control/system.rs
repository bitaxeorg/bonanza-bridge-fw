use bonanza_bridge_fw::{
    info, rx_stats,
    safety::{decode_wire_command, SafetyWireCommand},
};
use heapless::Vec;

use crate::pio_uart::buffered_rx_overflows;

use super::{CommandError, Controller, ControllerCommand};

#[derive(defmt::Format)]
pub enum Command {
    GetInfo,
    GetRxStats,
    GetSafetyStatus,
    ArmSafetyLease,
    SafetyHeartbeat,
    ClearSafetyFault,
    DisarmSafety,
}

impl Command {
    pub fn from_bytes(buf: &[u8]) -> Result<Self, CommandError> {
        match buf {
            [0x01] => Ok(Self::GetInfo),
            [0x02] => Ok(Self::GetRxStats),
            _ => match decode_wire_command(buf) {
                Some(SafetyWireCommand::GetStatus) => Ok(Self::GetSafetyStatus),
                Some(SafetyWireCommand::ArmLease) => Ok(Self::ArmSafetyLease),
                Some(SafetyWireCommand::Heartbeat) => Ok(Self::SafetyHeartbeat),
                Some(SafetyWireCommand::ClearFault) => Ok(Self::ClearSafetyFault),
                Some(SafetyWireCommand::Disarm) => Ok(Self::DisarmSafety),
                None => Err(CommandError::Invalid),
            },
        }
    }
}

impl ControllerCommand for Command {
    async fn handle(&self, controller: &mut Controller) -> Result<Vec<u8, 256>, CommandError> {
        match self {
            Command::GetInfo => info::firmware_info().map_err(|_| CommandError::Invalid),
            Command::GetRxStats => {
                let (pio_fifo_overflows, software_ring_overflows) = buffered_rx_overflows();
                let encoded = rx_stats::encode(pio_fifo_overflows, software_ring_overflows);
                Ok(Vec::from_slice(encoded.as_slice()).unwrap())
            }
            Command::GetSafetyStatus => Ok(controller.safety_status_payload()),
            Command::ArmSafetyLease => {
                controller.safety_arm().map_err(CommandError::from)?;
                Ok(controller.safety_status_payload())
            }
            Command::SafetyHeartbeat => {
                controller.safety_heartbeat().map_err(CommandError::from)?;
                Ok(controller.safety_status_payload())
            }
            Command::ClearSafetyFault => {
                controller.safety_clear_fault().map_err(CommandError::from)?;
                Ok(controller.safety_status_payload())
            }
            Command::DisarmSafety => {
                controller.safety_disarm();
                Ok(controller.safety_status_payload())
            }
        }
    }
}
