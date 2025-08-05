#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{gpio::{Input, Level, Output, Pull, Speed}, time::Hertz};
use embassy_stm32::spi::{Config, Spi};
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

use lr2021_apps::lr2021::status::Status;

/// Task to blink up to two leds
#[embassy_executor::task(pool_size = 2)]
async fn blink(mut led: Output<'static>, delay: u64) {

    loop {
        led.set_high();
        Timer::after_millis(delay).await;
        led.set_low();
        Timer::after_millis(delay).await;
    }
}


#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());
    info!("Starting get_temp");

    // Pin mapping
    // Name  | Connector | Nucleo
    // NRST  | CN8 A0    | PA0
    // SCK   | CN5 D13   | PA5
    // MISO  | CN5 D12   | PA6
    // MOSI  | CN5 D11   | PA7
    // NSS   | CN9 D7    | PA8
    // BUSY  | CN9 D3    | PB3
    // IRQ   | CN9 D2    | PA10
    // RFSW0 | CN9 D0    | PA3
    // RXSW1 | CN9 D1    | PA2
    // LEDTX | CN8 A5    | PC0
    // LEDRX | CN8 A4    | PC1

    let busy = Input::new(p.PB3, Pull::Up);
    let mut nreset = Output::new(p.PA0, Level::High, Speed::Low);
    nreset.set_low();
    Timer::after_millis(10).await;
    nreset.set_high();
    Timer::after_millis(10).await;
    info!("Reset done : busy = {}", busy.is_high());

    // Blink both TX/RX LEDs on the radio module to confirm PIN mapping
    let led_tx = Output::new(p.PC1, Level::High, Speed::Low);
    spawner.spawn(blink(led_tx, 500)).unwrap();

    let led_rx = Output::new(p.PC0, Level::High, Speed::Low);
    spawner.spawn(blink(led_rx, 133)).unwrap();

    // SPI
    let mut spi_config = Config::default();
    spi_config.frequency = Hertz(4_000_000);

    let mut spi = Spi::new_blocking(p.SPI1, p.PA5, p.PA7, p.PA6, spi_config);
    let mut nss = Output::new(p.PA8, Level::High, Speed::VeryHigh);

    // Get a temperature measurement every 15 seconds
    loop {
        Timer::after_secs(15).await;
        // Send a request with the opcode 0x125 corresponding to GetTemp
        // One byte parameter: 5:4 = source, b3 = format, 2:0 = resolution
        // Setting resolution to max (5 fractional bits) and format to degre Celsius
        let mut buf_req = [0x01,0x25,5|8];
        nss.set_low();
        unwrap!(spi.blocking_transfer_in_place(&mut buf_req));
        nss.set_high();
        let status = Status::from_slice(&buf_req);
        if !status.is_ok() {
            error!("Request => {=[u8]:x} =< {}", buf_req, status);
        }
        // Wait for busy to go down before reading the response
        while busy.is_high() {}
        // Read the 4 byte response
        let mut buf_rsp = [0x00;4];
        nss.set_low();
        unwrap!(spi.blocking_transfer_in_place(&mut buf_rsp));
        nss.set_high();
        let status = Status::from_slice(&buf_rsp);
        if status.is_ok() {
            info!("Temp = {}.{:02}", buf_rsp[2], (buf_rsp[3] as u16 * 100) >> 8);
        } else {
            error!("Request => {=[u8]:x} =< {}", buf_rsp, status);
        }
    }

}
