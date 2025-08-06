#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{exti::ExtiInput, gpio::{Input, Level, Output, Pull, Speed}, time::Hertz};
use embassy_stm32::spi::{Config, Spi};
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

use lr2021_apps::lr2021::Lr2021;

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

    // Blink both TX/RX LEDs on the radio module to confirm PIN mapping
    let led_tx = Output::new(p.PC1, Level::High, Speed::Low);
    spawner.spawn(blink(led_tx, 500)).unwrap();

    let led_rx = Output::new(p.PC0, Level::High, Speed::Low);
    spawner.spawn(blink(led_rx, 133)).unwrap();

    // Control pins
    let busy = Input::new(p.PB3, Pull::Up);
    let nreset = Output::new(p.PA0, Level::High, Speed::Low);
    let irq = ExtiInput::new(p.PA10, p.EXTI10, Pull::Up);

    // SPI
    let mut spi_config = Config::default();
    spi_config.frequency = Hertz(4_000_000);
    let spi = Spi::new_blocking(p.SPI1, p.PA5, p.PA7, p.PA6, spi_config);
    let nss = Output::new(p.PA8, Level::High, Speed::VeryHigh);

    let mut lr2021 = Lr2021::new(nreset, busy, irq, spi, nss);
    lr2021.reset().await;

    // Check version
    let mut buf_rsp = [0x00;4];
    match lr2021.cmd_rd(&[0x01,0x01], &mut buf_rsp).await {
        Ok(_) => info!("FW Version {:02x}.{:02x}", buf_rsp[2], buf_rsp[3]),
        Err(e) => error!("{}", e),
    }
    // Send a request with the opcode 0x125 corresponding to GetTemp
    // One byte parameter: 5:4 = source, b3 = format, 2:0 = resolution
    // Setting resolution to max (5 fractional bits) and format to degre Celsius
    let cmd = [0x01,0x25,5|8];
    // Get a temperature measurement every 15 seconds
    loop {
        Timer::after_secs(15).await;
        let mut buf_rsp = [0x00;4];
        match lr2021.cmd_rd(&cmd, &mut buf_rsp).await {
            Ok(_) => info!("Temp = {}.{:02}", buf_rsp[2], (buf_rsp[3] as u16 * 100) >> 8),
            Err(e) => error!("{}", e),
        }
    }

}
