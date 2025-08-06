use defmt::{debug, error, Format};
use embassy_stm32::{
    exti::ExtiInput,
    gpio::{Input, Output},
    mode::Blocking,
    spi::Spi,
};
use embassy_time::{Duration, Instant, Timer};
use status::Status;

pub mod status;
pub mod cmd_sys;

/// LR2021 Device
pub struct Lr2021 {
    // Pins
    nreset: Output<'static>,
    busy: Input<'static>,
    irq: ExtiInput<'static>,
    spi: Spi<'static, Blocking>,
    nss: Output<'static>,
    /// Last chip status
    status: Status,
}

/// Error using the LR2021
#[derive(Format)]
pub enum Lr2021Error {
    /// Unable to use SPI
    Spi,
    /// Last command failed
    CmdFail,
    /// Last command was invalid
    CmdErr,
    /// Timeout while waiting for busy
    BusyTimeout,
    /// Unknown error
    Unknown,
}

impl Lr2021 {
    /// Create a LR2021 Device
    pub fn new(
        nreset: Output<'static>,
        busy: Input<'static>,
        irq: ExtiInput<'static>,
        spi: Spi<'static, Blocking>,
        nss: Output<'static>,
    ) -> Self {
        Self {
            nreset,
            busy,
            irq,
            spi,
            nss,
            status: Status::default(),
        }
    }

    /// Reset the chip
    pub async fn reset(&mut self) {
        self.nreset.set_low();
        Timer::after_millis(10).await;
        self.nreset.set_high();
        Timer::after_millis(10).await;
        debug!("Reset done : busy = {}", self.busy.is_high());
    }

    /// Check if the busy pin is high (debug)
    pub fn is_busy(&self) -> bool {
        self.busy.is_high()
    }

    /// Last status (command status, chip mode, interrupt, ...)
    pub fn status(&self) -> &Status {
        &self.status
    }

    /// Write a command
    pub async fn cmd_wr(&mut self, req: &[u8]) -> Result<(), Lr2021Error> {
        self.nss.set_low();
        self.spi
            .blocking_transfer(self.status.as_mut(), req)
            .map_err(|_| Lr2021Error::Spi)?;
        self.nss.set_high();
        if !self.status.is_ok() {
            error!("Request failed: => {=[u8]:x} =< {}", self.status.as_bytes(), self.status);
        }
        self.status.check()
    }

    /// Write a command and read response
    /// Rsp must be n bytes equal to 0 where n is the number of expected byte
    pub async fn cmd_rd(&mut self, req: &[u8], rsp: &mut [u8]) -> Result<(), Lr2021Error> {
        self.cmd_wr(req).await?;
        // Wait for busy to go down before reading the response
        // TODO: add a timeout to avoid deadlock
        self.wait_ready(Duration::from_micros(250))?;
        self.nss.set_low();
        self.spi
            .blocking_transfer_in_place(rsp)
            .map_err(|_| Lr2021Error::Spi)?;
        self.nss.set_high();
        self.status.updt(&rsp[..2]);
        if !self.status.is_ok() {
            error!("Response => {=[u8]:x} =< {}", self.status.as_bytes(), self.status);
        }
        self.status.check()
    }

    /// Wait for busy to go low with timeout
    pub fn wait_ready(&self, timeout: Duration) -> Result<(), Lr2021Error> {
        let start = Instant::now();
        while self.busy.is_high() {
            if start.elapsed() >= timeout {
                return Err(Lr2021Error::BusyTimeout);
            }
        }
        Ok(())
    }

    /// Wait for an interrupt
    pub async fn wait_irq(&mut self) {
        if !self.irq.is_high() {
            self.irq.wait_for_rising_edge().await;
        }
    }
}
