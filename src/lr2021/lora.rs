use embedded_hal::digital::v2::OutputPin;
use embedded_hal_async::spi::SpiBus;

pub use super::cmd::cmd_lora::*;
use super::{cmd::cmd_regmem::write_reg_mem_mask32_cmd, system::set_additional_reg_to_retain_cmd, BusyPin, Lr2021, Lr2021Error};

impl<O,SPI, M> Lr2021<O,SPI, M> where
    O: OutputPin, SPI: SpiBus<u8>, M: BusyPin
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

    /// Set LoRa Syncword using legacy (SX127x) 1B notation: 0x34 for public network, 0x12 for private
    pub async fn set_lora_syncword(&mut self, syncword: u8) -> Result<(), Lr2021Error> {
        let req = set_lora_syncword_cmd(syncword);
        self.cmd_wr(&req).await
    }

    /// Set LoRa Syncword, using 2B notation (2 values on 5b each)
    /// Public network is (6,8) and private network is (2,4)
    pub async fn set_lora_syncword_ext(&mut self, s1: u8, s2: u8) -> Result<(), Lr2021Error> {
        let req = set_lora_syncword_extended_cmd(s1, s2);
        self.cmd_wr(&req).await
    }

    /// Return Information about last packet received
    pub async fn get_lora_packet_status(&mut self) -> Result<LoraPacketStatusRsp, Lr2021Error> {
        let req = get_lora_packet_status_req();
        let mut rsp = LoraPacketStatusRsp::new();
        self.cmd_rd(&req, rsp.as_mut()).await?;
        Ok(rsp)
    }

    /// Return extended Information about last packet received
    pub async fn get_lora_packet_status_adv(&mut self) -> Result<LoraPacketStatusRspAdv, Lr2021Error> {
        let req = get_lora_packet_status_req();
        let mut rsp = LoraPacketStatusRspAdv::new();
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

    /// Set LoRa Channel Activity Detection parameters
    /// - nb_symbols is the number of symbols for detection: between 1 and 15, use 4 for ideal performances.
    /// - pbl_any: set to false when explicitly searching for preamble, and 1 for any LoRa activity. Note that even when set to 0, CAD can still detect non-preamble, just less likely.
    /// - pnr_delta: Value between 0 and 15 to shorten the CAD time when there is obvisouly no LoRa activity. Set to 0 to always listen for the full duration, set to ~10 for optimal performances.
    ///   Higher value increase the chance to miss activity, while lower value will limit the chance to stop CAD early
    /// - exit_mode: Choose what happens after the CAD: fallback mode, RX or TX (for Listen-Before-Talk)
    /// - timeout: defines the timeout for the following RX or TX if exit mode is not CAD_ONLY
    /// - det_peak: control the detection threshold. Use None to let firmware automatically decide the threshold based on the SF/BW/nb_symbols/pnr_delta
    pub async fn set_lora_cad_params(&mut self, nb_symbols: u8, pbl_any: bool, pnr_delta: u8, exit_mode: ExitMode, timeout: u32, det_peak: Option<u8>) -> Result<(), Lr2021Error> {
        let req = set_lora_cad_params_cmd(nb_symbols, pbl_any, pnr_delta, exit_mode, timeout, det_peak.unwrap_or(0));
        let req_s = if det_peak.is_none() {&req[0..8]} else {&req};
        self.cmd_wr(req_s).await
    }

    /// Start a LoRa Channel Activity Detection (CAD)
    pub async fn set_lora_cad(&mut self) -> Result<(), Lr2021Error> {
        let req = set_lora_cad_cmd();
        self.cmd_wr(&req).await
    }


    const ADDR_LORA_PARAM : u32 = 0xF30A14;
    /// Enable compatibility with SX127x for SF6 communication and syncword format
    /// Must be called after each SetLoraModulation
    /// The retention enable allows to define a register slot to save this copatibility mode in retention
    pub async fn comp_sx127x_en(&mut self, ret_en: Option<u8>) -> Result<(), Lr2021Error> {
        let req = write_reg_mem_mask32_cmd(Self::ADDR_LORA_PARAM, 3<<18, 1<<19);
        self.cmd_wr(&req).await?;
        if let Some(slot) = ret_en {
            let req = set_additional_reg_to_retain_cmd(slot, Self::ADDR_LORA_PARAM);
            self.cmd_wr(&req).await?;
        }
        Ok(())
    }


}