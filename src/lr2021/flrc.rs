use embedded_hal::digital::v2::OutputPin;
use embedded_hal_async::spi::SpiBus;

pub use super::cmd::cmd_flrc::*;
use super::{BusyPin, Lr2021, Lr2021Error, PulseShape};

impl<O,SPI, M> Lr2021<O,SPI, M> where
    O: OutputPin, SPI: SpiBus<u8>, M: BusyPin
{

    /// Set Modulation parameters: raw bitrate, coding rate and pulse shaping
    pub async fn set_flrc_modulation(&mut self, bitrate: FlrcBitrate, cr: FlrcCr, pulse_shape: PulseShape) -> Result<(), Lr2021Error> {
        let req = set_flrc_modulation_params_cmd(bitrate, cr, pulse_shape);
        self.cmd_wr(&req).await
    }

    /// Set FLRC packet parameters: preamble, syncword, header implicit/explicit, CRC and packet length (max 511)
    pub async fn set_flrc_packet(&mut self, agc_pbl_len: AgcPblLen, sync_len: SyncLen, sync_tx: SyncTx, sync_match: SyncMatch, pkt_format: PktFormat, crc: Crc, pld_len: u16) -> Result<(), Lr2021Error> {
        let req = set_flrc_packet_params_cmd(agc_pbl_len, sync_len, sync_tx, sync_match, pkt_format, crc, pld_len);
        self.cmd_wr(&req).await
    }

    /// Configure one of the three possible syncword
    pub async fn set_flrc_syncword(&mut self, sw_num: u8, syncword: u32, is_16b: bool) -> Result<(), Lr2021Error> {
        let sw = if is_16b {syncword << 16} else {syncword};
        let req = set_flrc_syncword_cmd(sw_num, sw);
        let req_s = if is_16b {&req[..5]} else {&req};
        self.cmd_wr(req_s).await
    }

    /// Return length of last packet received
    pub async fn get_flrc_packet_status(&mut self) -> Result<FlrcPacketStatusRsp, Lr2021Error> {
        let req = get_flrc_packet_status_req();
        let mut rsp = FlrcPacketStatusRsp::new();
        self.cmd_rd(&req, rsp.as_mut()).await?;
        Ok(rsp)
    }

    /// Return basic RX stats
    pub async fn get_flrc_rx_stats(&mut self) -> Result<FlrcRxStatsRsp, Lr2021Error> {
        let req = get_flrc_rx_stats_req();
        let mut rsp = FlrcRxStatsRsp::new();
        self.cmd_rd(&req, rsp.as_mut()).await?;
        Ok(rsp)
    }

    /// Return advanced RX stats
    pub async fn get_flrc_rx_stats_adv(&mut self) -> Result<FlrcRxStatsRspAdv, Lr2021Error> {
        let req = get_flrc_rx_stats_req();
        let mut rsp = FlrcRxStatsRspAdv::new();
        self.cmd_rd(&req, rsp.as_mut()).await?;
        Ok(rsp)
    }

}