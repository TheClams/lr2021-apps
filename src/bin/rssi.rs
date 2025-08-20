#![no_std]
#![no_main]

// RSSI measurement across spectrum

use defmt::*;
use {defmt_rtt as _, panic_probe as _};

use embassy_executor::Spawner;
use embassy_stm32::{
    bind_interrupts, exti::ExtiInput, gpio::{Level, Output, Pull, Speed}, mode::Async, peripherals, spi::{Config as SpiConfig, Spi}, time::Hertz, usart::{self, Config as UartConfig, Uart}
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Timer};

use core::fmt::Write;
use heapless::String;

use lr2021_apps::{
    board::{blink, LedMode, SignalLedMode},
};
use lr2021::{
    radio::{PacketType, RxPath}, Lr2021, PulseShape, RxBw
};

const RF_MIN : u32 =  400_000_000;
const RF_MAX : u32 = 1100_000_000;
const RF_STEP: u32 =       50_000;
const RX_BW  : RxBw =  RxBw::Bw48;
const MEAS_US: u64 = 200;

/// Led modes
static LED_GREEN: SignalLedMode = Signal::new();
static LED_RED: SignalLedMode = Signal::new();

bind_interrupts!(struct UartIrqs {
    USART2 => usart::InterruptHandler<peripherals::USART2>;
});

pub type SignalData = Signal<CriticalSectionRawMutex, (u32,u16)>;
static DATA : SignalData = Signal::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());
    info!("Starting adsb_rx");

    // Start tasks to blink the leds
    let led_green = Output::new(p.PC0, Level::High, Speed::Low);
    spawner.spawn(blink(led_green, &LED_GREEN)).unwrap();
    LED_GREEN.signal(LedMode::BlinkSlow);

    let led_red = Output::new(p.PC1, Level::High, Speed::Low);
    spawner.spawn(blink(led_red, &LED_RED)).unwrap();
    LED_RED.signal(LedMode::Off);

    // Control pins
    let busy = ExtiInput::new(p.PB3, p.EXTI3, Pull::Up);
    let nreset = Output::new(p.PA0, Level::High, Speed::Low);

    // UART on Virtual Com: 115200bauds, 1 stop bit, no parity, no flow control
    let mut uart_config = UartConfig::default();
    uart_config.baudrate = 444_444;
    let uart = Uart::new(p.USART2, p.PA3, p.PA2, UartIrqs, p.DMA1_CH7, p.DMA1_CH6, uart_config).unwrap();

    // SPI
    let mut spi_config = SpiConfig::default();
    spi_config.frequency = Hertz(4_000_000);
    let spi = Spi::new(
        p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA1_CH3, p.DMA1_CH2, spi_config,
    );
    let nss = Output::new(p.PA8, Level::High, Speed::VeryHigh);

    // Create driver and reset board
    let mut lr2021 = Lr2021::new(nreset, busy, spi, nss);
    lr2021.reset().await.expect("Resetting chip !");

    // Initialize transceiver
    let mut rf = 400_000_000;
    lr2021.set_rf(rf).await.expect("SetRF");
    lr2021.set_rx_path(RxPath::LfPath, 0).await.expect("SetRxPath");
    // Frequencies are provided with a resolution 4MHz: calibration for 500, 700 and 900 MHz to cover the range we want observe
    lr2021.calib_fe(&[]).await.expect("Front-End calibration");
    // lr2021.calib_fe(&[125, 175, 225]).await.expect("Front-End calibration");

    match lr2021.get_status().await {
        Ok((status, intr)) => info!("Calibration Done: {} | {}", status, intr),
        Err(e) => warn!("Calibration Failed: {}", e),
    }

    // Configure demodulator for GFSK with modulation index 0.5
    lr2021.set_packet_type(PacketType::FskGeneric).await.expect("PacketTypeFsk");
    lr2021.set_fsk_modulation(RF_STEP, PulseShape::Bt0p5, RX_BW, RF_STEP>>3).await.expect("SetFskModulation");

    match lr2021.get_status().await {
        Ok((status, intr)) => info!("SetFsk Done: {} | {}", status, intr),
        Err(e) => error!("SetFsk Failed: {}", e),
    }

    // Setup radio to max gain (saturation unlikely in ADS-B and AGC might induce packet loss)
    lr2021.set_rx_gain(13).await.ok();
    lr2021.set_rx(0xFFFFFFFF, true).await.ok();

    // Configure RSSI for fine measurement
    let cfg_rssi = lr2021.rd_reg(0xF3014C).await.expect("GetRssiCfg");
    lr2021.wr_reg(0xF3014C, (cfg_rssi & 0xFFFFF0FF) | (7<<3)).await.expect("SetRssiCfg");

    // let mut s: String<32> = String::new();
    spawner.spawn(send_to_uart(uart, &DATA)).unwrap();
    loop {
        let rssi = lr2021.get_rssi_avg(Duration::from_micros(MEAS_US)).await.expect("RssiAvg");
        info!("RF {} : -{}dBm", rf, rssi>>1);
        // Wait for the UART to be ready
        while DATA.signaled() {
            Timer::after_micros(10).await;
        }
        DATA.signal((rf, rssi));
        rf += RF_STEP;
        if rf > RF_MAX {
            LED_RED.signal(LedMode::Flash);
            rf = RF_MIN;
        }
        lr2021.set_rf(rf).await.expect("SetRF");
    }
}

#[embassy_executor::task]
pub async fn send_to_uart(mut uart: Uart<'static, Async>, signal: &'static SignalData) {
    let mut s: String<32> = String::new();
    loop {
        // Wait for data to send
        let (rf, rssi) = signal.wait().await;
        // Create string "rf : rssi"
        s.clear();
        core::write!(&mut s, "{} : {}\r\n", rf, rssi).ok();
        // Send it on the uart
        uart.write(s.as_bytes()).await.ok();
    }
}