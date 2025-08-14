use defmt::{Format};
use embassy_time::{Duration, Instant, Timer};
use embedded_hal::digital::v2::{OutputPin, InputPin};
use embedded_hal_async::spi::SpiBus;
use status::{Intr, Status};

pub mod status;
pub mod system;
pub mod radio;
pub mod lora;
pub mod ble;
pub mod cmd;
pub mod flrc;

pub use cmd::{RxBw, PulseShape}; // Re-export Bandwidth enum as it is used for all packet types

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
        // debug!("[WR]  {=[u8]:x} ", req);
        self.nss.set_low().map_err(|_| Lr2021Error::Pin)?;
        self.spi
            .transfer(rsp_buf, req).await
            .map_err(|_| Lr2021Error::Spi)?;
        self.nss.set_high().map_err(|_| Lr2021Error::Pin)?;
        self.status().check()
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
        self.status().check()
    }

    /// Write a command
    pub async fn cmd_data(&mut self, mut opcode: [u8;2], buffer: &mut[u8]) -> Result<(), Lr2021Error> {
        self.nss.set_low().map_err(|_| Lr2021Error::Pin)?;
        // Send op-code followed by data
        self.spi
            .transfer_in_place(&mut opcode).await
            .map_err(|_| Lr2021Error::Spi)?;
        let status = Status::from_slice(&opcode);
        self.spi
            .transfer_in_place(buffer).await
            .map_err(|_| Lr2021Error::Spi)?;
        self.nss.set_high().map_err(|_| Lr2021Error::Pin)?;
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

    /// Wake-up the chip from a sleep mode (Set NSS low until busy goes low)
    pub async fn wake_up(&mut self) -> Result<(), Lr2021Error> {
        self.nss.set_low().map_err(|_| Lr2021Error::Pin)?;
        self.wait_ready(Duration::from_millis(100)).await?;
        self.nss.set_high().map_err(|_| Lr2021Error::Pin)
    }

}
