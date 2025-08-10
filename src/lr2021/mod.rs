use defmt::{debug, error, Format};
use embassy_time::{Duration, Instant, Timer};
use embedded_hal::digital::v2::{OutputPin, InputPin};
use embedded_hal_async::spi::SpiBus;
use status::{Intr, Status};

pub mod status;

// Re-export all cmd_
mod cmd;
pub use cmd::cmd_ble;
pub use cmd::cmd_bpsk;
pub use cmd::cmd_common;
pub use cmd::cmd_flrc;
pub use cmd::cmd_fsk;
pub use cmd::cmd_lora;
pub use cmd::cmd_lrfhss;
pub use cmd::cmd_ook;
pub use cmd::cmd_ranging;
pub use cmd::cmd_raw;
pub use cmd::cmd_regmem;
pub use cmd::cmd_system;
pub use cmd::cmd_wisun;
pub use cmd::cmd_zigbee;
pub use cmd::cmd_zwave;
pub use cmd::RxBw; // Re-export Bandwidth enum as it is used for all packet types

use cmd_lora::{Sf, LoraBw, LoraCr, Ldro, HeaderType};
use cmd_system::*;
use cmd_common::*;
use cmd_lora::*;

/// Chip Mode: Sleep/Standby/Fs/...
#[derive(Debug, Format, PartialEq)]
pub enum ChipMode {
    DeepSleep,
    Sleep(u32),
    Retention(u8,u32),
    StandbyRc,
    StandbyXosc,
    Fs,
    Tx,
    Rx,
}

/// LR2021 Device
pub struct Lr2021<I,O,SPI> {
    // Pins
    nreset: O,
    busy: I,
    spi: SPI,
    nss: O,
    /// Buffer to store SPI bytes from LR2021 when writing commands
    /// Size is set to largest command
    /// Could be re-purposed if needed, TBD
    buffer: [u8;18],
}

/// Error using the LR2021
#[derive(Format, Debug)]
pub enum Lr2021Error {
    /// Unable to Set/Get a pin level
    Pin,
    /// Unable to use SPI
    Spi,
    /// Last command failed
    CmdFail,
    /// Last command was invalid
    CmdErr,
    /// Timeout while waiting for busy
    BusyTimeout,
    /// Command with invalid size (>18B)
    InvalidSize,
    /// Unknown error
    Unknown,
}

impl<I,O,SPI> Lr2021<I,O,SPI> where
    I: InputPin, O: OutputPin, SPI: SpiBus<u8>
{
    /// Create a LR2021 Device
    pub fn new(nreset: O, busy: I, spi: SPI, nss: O) -> Self {
        Self { nreset, busy, spi, nss, buffer: [0;18]}
    }

    /// Reset the chip
    pub async fn reset(&mut self) -> Result<(), Lr2021Error> {
        self.nreset.set_low().map_err(|_| Lr2021Error::Pin)?;
        Timer::after_millis(10).await;
        self.nreset.set_high().map_err(|_| Lr2021Error::Pin)?;
        Timer::after_millis(10).await;
        debug!("Reset done");
        Ok(())
    }

    /// Check if the busy pin is high (debug)
    pub fn is_busy(&self) -> bool {
        self.busy.is_high().unwrap_or(false)
    }

    /// Last status (command status, chip mode, interrupt, ...)
    pub fn status(&self) -> Status {
        Status::from_slice(&self.buffer[..2])
    }

    /// Last captured interrupt status
    /// Note: might be incomplete if last command was less than 6 bytes
    pub fn last_intr(&self) -> Intr {
        Intr::from_slice(&self.buffer[2..6])
    }

    /// Write a command
    pub async fn cmd_wr(&mut self, req: &[u8]) -> Result<(), Lr2021Error> {
        if req.len() > 18 {
            return Err(Lr2021Error::InvalidSize);
        }
        let rsp_buf = &mut self.buffer[..req.len()];
        self.nss.set_low().map_err(|_| Lr2021Error::Pin)?;
        self.spi
            .transfer(rsp_buf, req).await
            .map_err(|_| Lr2021Error::Spi)?;
        self.nss.set_high().map_err(|_| Lr2021Error::Pin)?;
        let status = self.status();
        if !status.is_ok() {
            error!("Request failed: => {=[u8]:x} =< {}", self.buffer[..req.len()], status);
        }
        status.check()
    }

    /// Write a command and read response
    /// Rsp must be n bytes equal to 0 where n is the number of expected byte
    pub async fn cmd_rd(&mut self, req: &[u8], rsp: &mut [u8]) -> Result<(), Lr2021Error> {
        self.cmd_wr(req).await?;
        // Wait for busy to go down before reading the response
        // Some command can have large delay: temperature measurement with highest resolution (13b) takes more than 270us
        self.wait_ready(Duration::from_millis(1)).await?;
        // Read response by transfering a buffer full of 0 and replacing it by the read bytes
        self.nss.set_low().map_err(|_| Lr2021Error::Pin)?;
        self.spi
            .transfer_in_place(rsp).await
            .map_err(|_| Lr2021Error::Spi)?;
        self.nss.set_high().map_err(|_| Lr2021Error::Pin)?;
        // Save the first 2 byte in case we want to access status information
        self.buffer[..2].copy_from_slice(&rsp[..2]);
        let status = self.status();
        if !status.is_ok() {
            error!("Response => {=[u8]:x} =< {}", rsp, status);
        }
        status.check()
    }

    /// Write a command
    pub async fn cmd_data(&mut self, mut opcode: [u8;2], buffer: &mut[u8]) -> Result<(), Lr2021Error> {
        self.nss.set_low().map_err(|_| Lr2021Error::Pin)?;
        // Send op-code followed by data
        self.spi
            .transfer_in_place(&mut opcode).await
            .map_err(|_| Lr2021Error::Spi)?;
        let status = Status::from_slice(&opcode);
        if !status.is_ok() {
            error!("Previous command failed: {}", status);
        }
        self.spi
            .transfer_in_place(buffer).await
            .map_err(|_| Lr2021Error::Spi)?;
        self.nss.set_high().map_err(|_| Lr2021Error::Pin)?;
        status.check()
    }

    /// Write a command
    pub async fn wr_tx_fifo(&mut self, buffer: &mut[u8]) -> Result<(), Lr2021Error> {
        self.cmd_data([0,2], buffer).await
    }

    pub async fn rd_rx_fifo(&mut self, buffer: &mut[u8]) -> Result<(), Lr2021Error> {
        self.cmd_data([0,1], buffer).await
    }

    /// Wait for busy to go low with timeout
    pub async fn wait_ready(&self, timeout: Duration) -> Result<(), Lr2021Error> {
        let start = Instant::now();
        while self.busy.is_high().map_err(|_| Lr2021Error::Pin)? {
            if start.elapsed() >= timeout {
                return Err(Lr2021Error::BusyTimeout);
            }
            Timer::after_micros(5).await;
        }
        Ok(())
    }

    /// Read status and interrupt from the chip
    pub async fn get_status(&mut self) -> Result<(Status,Intr), Lr2021Error> {
        let req = get_status_req();
        let mut rsp = StatusRsp::new();
        self.cmd_rd(&req, rsp.as_mut()).await?;
        Ok((rsp.status(), rsp.intr()))
    }

    /// Read status and interrupt from the chip
    pub async fn get_version(&mut self) -> Result<VersionRsp, Lr2021Error> {
        let req = get_version_req();
        let mut rsp = VersionRsp::new();
        self.cmd_rd(&req, rsp.as_mut()).await?;
        Ok(rsp)
    }

    /// Read interrupt from the chip and clear them all
    pub async fn get_and_clear_irq(&mut self) -> Result<Intr, Lr2021Error> {
        let req = get_and_clear_irq_req();
        let mut rsp = StatusRsp::new();
        self.cmd_rd(&req, rsp.as_mut()).await?;
        Ok(rsp.intr())
    }

    /// Set the RF channel (in Hz)
    pub async fn clear_irqs(&mut self, intr: Intr) -> Result<(), Lr2021Error> {
        let req = clear_irq_cmd(intr.value());
        self.cmd_wr(&req).await
    }

    /// Set the RF channel (in Hz)
    pub async fn set_rf(&mut self, freq: u32) -> Result<(), Lr2021Error> {
        let req = set_rf_frequency_cmd(freq);
        self.cmd_wr(&req).await
    }

    /// Set the RX Path (LF/HF)
    pub async fn set_rx_path(&mut self, rx_path: RxPath, rx_boost: u8) -> Result<(), Lr2021Error> {
        let req = cmd_common::set_rx_path_adv_cmd(rx_path, rx_boost);
        self.cmd_wr(&req).await
    }

    /// Run calibration on up to 3 frequencies on 16b (MSB encode RX Path)
    /// If none, use current frequency
    pub async fn calib_fe(&mut self, freqs_4m: &[u16]) -> Result<(), Lr2021Error> {
        let f0 = freqs_4m.first().map(|&f| f).unwrap_or(0);
        let f1 = freqs_4m.get(1).map(|&f| f).unwrap_or(0);
        let f2 = freqs_4m.get(2).map(|&f| f).unwrap_or(0);
        let req = calib_fe_cmd(f0,f1,f2);
        let len = 2 + 2*freqs_4m.len();
        self.cmd_wr(&req[..len]).await
    }

    /// Set the packet type
    pub async fn set_packet_type(&mut self, packet_type: PacketType) -> Result<(), Lr2021Error> {
        let req = cmd_common::set_packet_type_cmd(packet_type);
        self.cmd_wr(&req).await
    }

    /// Set LoRa Modulation parameters
    pub async fn set_lora_modulation(&mut self, sf: Sf, bw: LoraBw, cr: LoraCr, ldro: Ldro) -> Result<(), Lr2021Error> {
        let req = cmd_lora::set_lora_modulation_params_cmd(sf, bw, cr, ldro);
        self.cmd_wr(&req).await
    }

    /// Set LoRa Packet parameters
    pub async fn set_lora_packet(&mut self, pbl_len: u16, payload_len: u8, header_type: HeaderType, crc_en: bool, invert_iq: bool) -> Result<(), Lr2021Error> {
        let req = cmd_lora::set_lora_packet_params_cmd(pbl_len, payload_len, header_type, crc_en, invert_iq);
        self.cmd_wr(&req).await
    }

    /// Set Tx power and ramp time
    pub async fn set_tx_params(&mut self, tx_power: u8, ramp_time: RampTime) -> Result<(), Lr2021Error> {
        let req = cmd_common::set_tx_params_cmd(tx_power, ramp_time);
        self.cmd_wr(&req).await
    }

    /// Set chip in RX mode
    pub async fn set_rx(&mut self, rx_timeout: u32, wait_ready: bool) -> Result<(), Lr2021Error> {
        let req = cmd_common::set_rx_adv_cmd(rx_timeout);
        self.cmd_wr(&req).await?;
        if wait_ready {
            self.wait_ready(Duration::from_millis(100)).await?;
        }
        Ok(())
    }

    /// Set chip in TX mode
    pub async fn set_tx(&mut self, tx_timeout: u32) -> Result<(), Lr2021Error> {
        let req = cmd_common::set_tx_adv_cmd(tx_timeout);
        self.cmd_wr(&req).await
    }

    /// Set Tx power and ramp time
    pub async fn set_chip_mode(&mut self, chip_mode: ChipMode) -> Result<(), Lr2021Error> {
        match chip_mode {
            ChipMode::DeepSleep      => self.cmd_wr(&set_sleep_cmd(false, 0)).await,
            ChipMode::Sleep(t)       => self.cmd_wr(&set_sleep_adv_cmd(true, 0, t)).await,
            ChipMode::Retention(r,t) => self.cmd_wr(&set_sleep_adv_cmd(true, r, t)).await,
            ChipMode::StandbyRc   => self.cmd_wr(&set_standby_cmd(StandbyMode::Rc)).await,
            ChipMode::StandbyXosc => self.cmd_wr(&set_standby_cmd(StandbyMode::Xosc)).await,
            ChipMode::Fs => self.cmd_wr(&set_fs_cmd()).await,
            ChipMode::Tx => self.cmd_wr(&set_tx_cmd()).await,
            ChipMode::Rx => self.cmd_wr(&set_rx_cmd()).await,
        }
    }

    /// Wake-up the chip from a sleep mode (Set NSS low until busy goes low)
    pub async fn wake_up(&mut self) -> Result<(), Lr2021Error> {
        self.nss.set_low().map_err(|_| Lr2021Error::Pin)?;
        self.wait_ready(Duration::from_millis(100)).await?;
        self.nss.set_high().map_err(|_| Lr2021Error::Pin)
    }

    /// Configure a pin as IRQ and enable interrupts for this pin
    pub async fn set_dio_irq(&mut self, dio: u8, intr_en: Intr) -> Result<(), Lr2021Error> {
        let sleep_pull = if dio > 6 {PullDrive::PullAuto} else {PullDrive::PullUp};
        let req = cmd_system::set_dio_function_cmd(dio, DioFunc::Irq, sleep_pull);
        self.cmd_wr(&req).await?;
        let req = cmd_system::set_dio_irq_config_cmd(dio, intr_en.value());
        self.cmd_wr(&req).await
    }

    /// Clear RX stats
    pub async fn clear_rx_stats(&mut self) -> Result<(), Lr2021Error> {
        self.cmd_wr(&reset_rx_stats_cmd()).await
    }

    /// Clear RX Fifo
    pub async fn clear_rx_fifo(&mut self) -> Result<(), Lr2021Error> {
        self.cmd_wr(&clear_rx_fifo_cmd()).await
    }

    /// Clear TX Fifo
    pub async fn clear_tx_fifo(&mut self) -> Result<(), Lr2021Error> {
        self.cmd_wr(&clear_tx_fifo_cmd()).await
    }

    /// Return number of byte in TX FIFO
    pub async fn get_tx_fifo_lvl(&mut self) -> Result<u16, Lr2021Error> {
        let req = get_tx_fifo_level_req();
        let mut rsp = TxFifoLevelRsp::new();
        self.cmd_rd(&req, rsp.as_mut()).await?;
        Ok(rsp.level())
    }

    /// Return number of byte in RX FIFO
    pub async fn get_rx_fifo_lvl(&mut self) -> Result<u16, Lr2021Error> {
        let req = get_rx_fifo_level_req();
        let mut rsp = RxFifoLevelRsp::new();
        self.cmd_rd(&req, rsp.as_mut()).await?;
        Ok(rsp.level())
    }

    /// Return length of last packet received
    pub async fn get_rx_pkt_len(&mut self) -> Result<u16, Lr2021Error> {
        let req = get_rx_pkt_length_req();
        let mut rsp = RxPktLengthRsp::new();
        self.cmd_rd(&req, rsp.as_mut()).await?;
        Ok(rsp.pkt_length())
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
