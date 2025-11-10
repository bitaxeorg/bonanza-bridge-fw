use embassy_rp::adc::{Adc, Channel};
use heapless::Vec;

use super::CommandError;

pub struct Pins<'d> {
    pub adc: Adc<'d, embassy_rp::adc::Async>,
    pub domain1: Channel<'d>,
    pub domain2: Channel<'d>,
    pub domain3: Channel<'d>,
}

#[derive(defmt::Format)]
pub enum Command {
    ReadDomain1, // 0x50
    ReadDomain2, // 0x51
    ReadDomain3, // 0x52
}

impl Command {
    pub fn from_bytes(buf: &[u8]) -> Result<Self, CommandError> {
        //defmt::println!("ADC COMMAND {:x}", buf);
        match buf {
            [0x50] => Ok(Self::ReadDomain1),
            [0x51] => Ok(Self::ReadDomain2),
            [0x52] => Ok(Self::ReadDomain3),
            _ => Err(CommandError::Invalid),
        }
    }
}

impl super::ControllerCommand for Command {
    async fn handle(&self, controller: &mut super::Controller) -> Result<Vec<u8, 256>, CommandError> {
        let adc = &mut controller.adc.adc;
        let value = match self {
            Command::ReadDomain1 => adc.read(&mut controller.adc.domain1).await,
            Command::ReadDomain2 => adc.read(&mut controller.adc.domain2).await,
            Command::ReadDomain3 => adc.read(&mut controller.adc.domain3).await,
        }
        .map_err(|_| CommandError::Message("ADC Read Error"))?;

        Ok(Vec::from_slice(&value.to_le_bytes()).unwrap())
    }
}
