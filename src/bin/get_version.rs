#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{exti::ExtiInput, gpio::{Input, Level, Output, Pull, Speed}, time::Hertz};
use embassy_stm32::spi::{Config, Spi};
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

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
    info!("Starting get_version");

    // Get an interrupt on the button pin to wait
    let mut button = ExtiInput::new(p.PC13, p.EXTI13, Pull::Up);

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
    spawner.spawn(blink(led_rx, 125)).unwrap();

    // SPI
    let mut spi_config = Config::default();
    spi_config.frequency = Hertz(4_000_000);

    let mut spi = Spi::new_blocking(p.SPI1, p.PA5, p.PA7, p.PA6, spi_config);
    let mut nss = Output::new(p.PA8, Level::High, Speed::VeryHigh);

    // Request the chip version when the button is pressed
    loop {
        button.wait_for_low().await;
        // Send a request with the opcode 0x101 corresponding to GetVersion
        let mut buf_req = [0x01,0x01];
        nss.set_low();
        unwrap!(spi.blocking_transfer_in_place(&mut buf_req));
        nss.set_high();
        info!("Request => {=[u8]:x}", buf_req);
        // Wait for busy to go down before reading the response
        while busy.is_high() {}
        // Read the 4 byte response
        let mut buf_rsp = [0x00;4];
        nss.set_low();
        unwrap!(spi.blocking_transfer_in_place(&mut buf_rsp));
        nss.set_high();
        info!("Response =>  {=[u8]:x}", buf_rsp);
        // Wait for button release
        button.wait_for_high().await;
    }

}
