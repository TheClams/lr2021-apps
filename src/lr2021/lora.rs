use embedded_hal::digital::v2::{OutputPin, InputPin};
use embedded_hal_async::spi::SpiBus;

pub use super::cmd::cmd_lora::*;
use super::{Lr2021, Lr2021Error};

impl<I,O,SPI> Lr2021<I,O,SPI> where
    I: InputPin, O: OutputPin, SPI: SpiBus<u8>
{

    /// Set LoRa Modulation parameters
    pub async fn set_lora_modulation(&mut self, sf: Sf, bw: LoraBw, cr: LoraCr, ldro: Ldro) -> Result<(), Lr2021Error> {
        let req = set_lora_modulation_params_cmd(sf, bw, cr, ldro);
        self.cmd_wr(&req).await
    }

    /// Set LoRa Packet parameters
    pub async fn set_lora_packet(&mut self, pbl_len: u16, payload_len: u8, header_type: HeaderType, crc_en: bool, invert_iq: bool) -> Result<(), Lr2021Error> {
        let req = set_lora_packet_params_cmd(pbl_len, payload_len, header_type, crc_en, invert_iq);
        self.cmd_wr(&req).await
    }

    /// Return length of last packet received
    pub async fn get_lora_packet_status(&mut self) -> Result<LoraPacketStatusRsp, Lr2021Error> {
        let req = get_lora_packet_status_req();
        let mut rsp = LoraPacketStatusRsp::new();
        self.cmd_rd(&req, rsp.as_mut()).await?;
        Ok(rsp)
    }

    /// Return length of last packet received
    pub async fn get_lora_rx_stats(&mut self) -> Result<LoraRxStatsRsp, Lr2021Error> {
        let req = get_lora_rx_stats_req();
        let mut rsp = LoraRxStatsRsp::new();
        self.cmd_rd(&req, rsp.as_mut()).await?;
        Ok(rsp)
    }

}