use embassy_time::Duration;
use embedded_hal::digital::v2::OutputPin;
use embedded_hal_async::spi::SpiBus;

pub use super::cmd::cmd_common::*;
use super::{BusyPin, Lr2021, Lr2021Error};

impl<O,SPI, M> Lr2021<O,SPI, M> where
    O: OutputPin, SPI: SpiBus<u8>, M: BusyPin
{

    /// Set the RF channel (in Hz)
    pub async fn set_rf(&mut self, freq: u32) -> Result<(), Lr2021Error> {
        let req = set_rf_frequency_cmd(freq);
        self.cmd_wr(&req).await
    }

    /// Set the RX Path (LF/HF)
    pub async fn set_rx_path(&mut self, rx_path: RxPath, rx_boost: u8) -> Result<(), Lr2021Error> {
        let req = set_rx_path_adv_cmd(rx_path, rx_boost);
        self.cmd_wr(&req).await
    }

    /// Set the packet type
    pub async fn set_packet_type(&mut self, packet_type: PacketType) -> Result<(), Lr2021Error> {
        let req = set_packet_type_cmd(packet_type);
        self.cmd_wr(&req).await
    }

    /// Set Tx power and ramp time
    pub async fn set_tx_params(&mut self, tx_power: u8, ramp_time: RampTime) -> Result<(), Lr2021Error> {
        let req = set_tx_params_cmd(tx_power, ramp_time);
        self.cmd_wr(&req).await
    }

    /// Configure LF Power Amplifier
    pub async fn set_pa_lf(&mut self, pa_lf_mode: PaLfMode, pa_lf_duty_cycle: u8, pa_lf_slices: u8) -> Result<(), Lr2021Error> {
        let req = set_pa_config_cmd(PaSel::LfPa, pa_lf_mode, pa_lf_duty_cycle, pa_lf_slices);
        self.cmd_wr(&req).await
    }

    /// Configure HF Power Amplifier
    pub async fn set_pa_hf(&mut self) -> Result<(), Lr2021Error> {
        let req = set_pa_config_cmd(PaSel::HfPa, PaLfMode::LfPaFsm, 6, 7);
        self.cmd_wr(&req).await
    }

    /// Set the Fallback mode after TX/RX
    pub async fn set_fallback(&mut self, fallback_mode: FallbackMode) -> Result<(), Lr2021Error> {
        let req = set_rx_tx_fallback_mode_cmd(fallback_mode);
        self.cmd_wr(&req).await
    }

    /// Set chip in TX mode. Set timeout to 0 or to a value longer than the packet duration.
    pub async fn set_tx(&mut self, tx_timeout: u32) -> Result<(), Lr2021Error> {
        let req = set_tx_adv_cmd(tx_timeout);
        self.cmd_wr(&req).await
    }

    /// Set chip in RX mode. A timeout equal to 0 means a single reception, the value 0xFFFFFF is for continuous RX (i.e. always restart reception)
    /// and any other value, the chip will go back to its fallback mode if a reception does not occur before the timeout is elapsed
    pub async fn set_rx(&mut self, rx_timeout: u32, wait_ready: bool) -> Result<(), Lr2021Error> {
        let req = set_rx_adv_cmd(rx_timeout);
        self.cmd_wr(&req).await?;
        if wait_ready {
            self.wait_ready(Duration::from_millis(100)).await?;
        }
        Ok(())
    }

    /// Clear RX stats
    pub async fn clear_rx_stats(&mut self) -> Result<(), Lr2021Error> {
        self.cmd_wr(&reset_rx_stats_cmd()).await
    }

    /// Return length of last packet received
    pub async fn get_rx_pkt_len(&mut self) -> Result<u16, Lr2021Error> {
        let req = get_rx_pkt_length_req();
        let mut rsp = RxPktLengthRsp::new();
        self.cmd_rd(&req, rsp.as_mut()).await?;
        Ok(rsp.pkt_length())
    }

}
