#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{gpio::{Input, Level, Output, Pull, Speed}, time::Hertz};
use embassy_stm32::spi::{Config as SpiConfig, Spi};
use embassy_stm32::usart::{Config as UartConfig, Uart};
use embassy_stm32::{bind_interrupts, peripherals, usart};
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

use core::fmt::Write;
use heapless::String;

use lr2021::{system::{self, AdcRes, TempSrc}, Lr2021};

bind_interrupts!(struct Irqs {
    USART2 => usart::InterruptHandler<peripherals::USART2>;
});

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
    info!("Starting get_temp (v4)");

    // Blink both TX/RX LEDs on the radio module to confirm PIN mapping
    let led_tx = Output::new(p.PC1, Level::High, Speed::Low);
    spawner.spawn(blink(led_tx, 500)).unwrap();

    let led_rx = Output::new(p.PC0, Level::High, Speed::Low);
    spawner.spawn(blink(led_rx, 133)).unwrap();

    // Control pins
    let busy = Input::new(p.PB3, Pull::Up);
    let nreset = Output::new(p.PA0, Level::High, Speed::Low);

    // SPI
    let mut spi_config = SpiConfig::default();
    spi_config.frequency = Hertz(4_000_000);
    let spi = Spi::new(p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA1_CH3, p.DMA1_CH2, spi_config);
    let nss = Output::new(p.PA8, Level::High, Speed::VeryHigh);

    // UART on Virtual Com: 115200bauds, 1 stop bit, no parity, no flow control
    let uart_config = UartConfig::default();
    let mut uart = Uart::new(p.USART2, p.PA3, p.PA2, Irqs, p.DMA1_CH7, p.DMA1_CH6, uart_config).unwrap();

    let mut lr2021 = Lr2021::new_blocking(nreset, busy, spi, nss);
    lr2021.reset().await
        .unwrap_or_else(|_| error!("Unable to reset chip !"));

    // String buffer
    let mut s: String<128> = String::new();

    // Check version
    let mut fw_version = system::VersionRsp::new();
    match lr2021.cmd_rd(&system::get_version_req(), fw_version.as_mut()).await {
        Ok(_) => {
            info!("FW Version {:02x}.{:02x}", fw_version.major(), fw_version.minor());
            core::write!(&mut s, "FW Version {}.{}!\r\n", fw_version.major(), fw_version.minor()).unwrap();
            uart.write(s.as_bytes()).await.ok();
        }
        Err(e) => error!("{}", e),
    }

    // Report status
    match lr2021.get_status().await {
        Ok((status,intr)) => info!("{} | Intr={:08x}", status, intr.value()),
        Err(e) => error!("{}", e),
    }

    // Get periodic temperature measurement
    loop {
        Timer::after_secs(10).await;
        match lr2021.get_temperature(TempSrc::Vbe, AdcRes::Res13bit).await {
            Ok(t) => {
                info!("{}.{:02}", t >> 5, ((t&31) * 100) >> 5);
                s.clear();
                match core::write!(&mut s, "T = {}\r\n", t) {
                    Ok(_) => {uart.write(s.as_bytes()).await.ok();}
                    Err(_) => {error!("Unable to write string");}
                }

            }
            Err(e) => error!("{}", e),
        }
    }

}
