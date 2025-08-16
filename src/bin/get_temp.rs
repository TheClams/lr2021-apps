#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{gpio::{Input, Level, Output, Pull, Speed}, time::Hertz};
use embassy_stm32::spi::{Config, Spi};
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

use lr2021::{system::{self, AdcRes, TempSrc}, Lr2021};

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
    info!("Starting get_temp (v3)");

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

    // SPI
    let mut spi_config = Config::default();
    spi_config.frequency = Hertz(4_000_000);
    let spi = Spi::new(p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA1_CH3, p.DMA1_CH2, spi_config);
    let nss = Output::new(p.PA8, Level::High, Speed::VeryHigh);

    let mut lr2021 = Lr2021::new_blocking(nreset, busy, spi, nss);
    lr2021.reset().await
        .unwrap_or_else(|_| error!("Unable to reset chip !"));

    // Check version
    let mut fw_version = system::VersionRsp::new();
    match lr2021.cmd_rd(&system::get_version_req(), fw_version.as_mut()).await {
        Ok(_) => info!("FW Version {:02x}.{:02x}", fw_version.major(), fw_version.minor()),
        Err(e) => error!("{}", e),
    }

    // Report status
    match lr2021.get_status().await {
        Ok((status,intr)) => info!("{} | Intr={:08x}", status, intr.value()),
        Err(e) => error!("{}", e),
    }

    // Create the GetTemp command once
    let cmd = system::get_temp_req(TempSrc::Vbe, AdcRes::Res13bit);

    // Get a temperature measurement every 15 seconds
    loop {
        Timer::after_secs(15).await;
        let mut temp = system::TempRsp::new();
        match lr2021.cmd_rd(&cmd, temp.as_mut()).await {
            Ok(_) => info!("Temp = {}", temp),
            Err(e) => error!("{}", e),
        }
    }

}
