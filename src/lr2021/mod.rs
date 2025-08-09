use defmt::{debug, error, Format};
use embassy_time::{Duration, Instant, Timer};
use embedded_hal::digital::v2::{OutputPin, InputPin};
use embedded_hal_async::{digital::Wait, spi::SpiBus};
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
pub use cmd::RxBw;

use self::cmd_common::PacketType;
use self::cmd_common::RxPath; // Re-export Bandwidth enum as it is used for all packet types

/// LR2021 Device
pub struct Lr2021<I,O,IRQ,SPI> {
    // Pins
    nreset: O,
    busy: I,
    irq: IRQ,
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

impl<I,O,IRQ,SPI> Lr2021<I,O,IRQ,SPI> where
    I: InputPin, O: OutputPin, IRQ: InputPin + Wait, SPI: SpiBus<u8>
{
    /// Create a LR2021 Device
    pub fn new(
        nreset: O,
        busy: I,
        irq: IRQ,
        spi: SPI,
        nss: O,
    ) -> Self {
        Self {
            nreset,
            busy,
            irq,
            spi,
            nss,
            buffer: [0;18],
        }
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

    /// Wait for an interrupt
    pub async fn wait_irq(&mut self) -> Result<(), Lr2021Error> {
        if !self.irq.is_high().map_err(|_| Lr2021Error::Pin)? {
            self.irq.wait_for_rising_edge().await
                .map_err(|_| Lr2021Error::Pin)?;
        }
        Ok(())
    }

    /// Read status and interrupt from the chip
    pub async fn get_status(&mut self) -> Result<(Status,Intr), Lr2021Error> {
        let req = cmd_system::get_status_req();
        let mut rsp = cmd_system::GetStatusRsp::new();
        self.cmd_rd(&req, rsp.as_mut()).await?;
        Ok((rsp.status(), rsp.intr()))
    }

    /// Read status and interrupt from the chip
    pub async fn get_version(&mut self) -> Result<cmd_system::GetVersionRsp, Lr2021Error> {
        let req = cmd_system::get_version_req();
        let mut rsp = cmd_system::GetVersionRsp::new();
        self.cmd_rd(&req, rsp.as_mut()).await?;
        Ok(rsp)
    }

    /// Read interrupt from the chip and clear them all
    pub async fn get_and_clear_irq(&mut self) -> Result<Intr, Lr2021Error> {
        let req = cmd_system::get_and_clear_irq_req();
        let mut rsp = cmd_system::GetStatusRsp::new();
        self.cmd_rd(&req, rsp.as_mut()).await?;
        Ok(rsp.intr())
    }

    /// Set the RF channel (in Hz)
    pub async fn set_rf(&mut self, freq: u32) -> Result<(), Lr2021Error> {
        let req = cmd_common::set_rf_frequency_cmd(freq);
        self.cmd_wr(&req).await?;
        Ok(())
    }

    /// Set the RX Path (LF/HF)
    pub async fn set_rx_path(&mut self, rx_path: RxPath, rx_boost: u8) -> Result<(), Lr2021Error> {
        let req = cmd_common::set_rx_path_adv_cmd(rx_path, rx_boost);
        self.cmd_wr(&req).await?;
        Ok(())
    }

    /// Run calibration on up to 3 frequencies on 16b (MSB encode RX Path)
    /// If none, use current frequency
    pub async fn calib_fe(&mut self, freqs_4m: &[u16]) -> Result<(), Lr2021Error> {
        let f0 = freqs_4m.first().map(|&f| f).unwrap_or(0);
        let f1 = freqs_4m.get(1).map(|&f| f).unwrap_or(0);
        let f2 = freqs_4m.get(2).map(|&f| f).unwrap_or(0);
        let req = cmd_system::calib_fe_cmd(f0,f1,f2);
        let len = 2 + 2*freqs_4m.len();
        self.cmd_wr(&req[..len]).await?;
        Ok(())
    }

    /// Set the packet type
    pub async fn set_packet_type(&mut self, packet_type: PacketType) -> Result<(), Lr2021Error> {
        let req = cmd_common::set_packet_type_cmd(packet_type);
        self.cmd_wr(&req).await?;
        Ok(())
    }

    /// Set the packet type
    pub async fn set_lora_modulation(&mut self, packet_type: PacketType) -> Result<(), Lr2021Error> {
        let req = setlora;
        self.cmd_wr(&req).await?;
        Ok(())
    }
}
