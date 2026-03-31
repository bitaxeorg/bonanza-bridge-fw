use super::CommandError;
use heapless::Vec;

pub struct Pins<'d> {
    pub v5_en: embassy_rp::gpio::Output<'d>,
    pub asic_rst: embassy_rp::gpio::Output<'d>,
    pub asic_trip: embassy_rp::gpio::Input<'d>,
}

#[derive(defmt::Format)]
pub enum Command {
    Set5vEn { level: bool },
    Get5vEn,

    SetAsicRst { level: bool },
    GetAsicRst,

    GetAsicTrip,
}

impl Command {
    pub fn from_bytes(buf: &[u8]) -> Result<Self, CommandError> {
        defmt::println!("GETTING GPIO COMMAND FROM BYTES {:x}", buf);
        match buf {
            // Get 5V Enable
            [0x01] => Ok(Self::Get5vEn),
            // Set 5V Enable
            [0x01, level] => Ok(Self::Set5vEn { level: *level > 0 }),
            // Get ASIC Reset
            [0x02] => Ok(Self::GetAsicRst),
            // Set ASIC Reset
            [0x02, level] => Ok(Self::SetAsicRst { level: *level > 0 }),
            // Get ASIC Trip
            [0x03] => Ok(Self::GetAsicTrip),
            _ => Err(CommandError::Invalid),
        }
    }
}

impl super::ControllerCommand for Command {
    async fn handle(&self, controller: &mut super::Controller) -> Result<Vec<u8, 256>, CommandError> {
        let level = match self {
            Command::Get5vEn => bool::from(controller.gpio.v5_en.get_output_level()),
            Command::Set5vEn { level } => {
                controller.gpio.v5_en.set_level((*level).into());
                *level
            }
            Command::GetAsicRst => bool::from(controller.gpio.asic_rst.get_output_level()),
            Command::SetAsicRst { level } => {
                controller.gpio.asic_rst.set_level((*level).into());
                *level
            }
            Command::GetAsicTrip => controller.gpio.asic_trip.is_high(),
        };

        Ok(Vec::from_slice(&[level as u8]).unwrap())
    }
}
