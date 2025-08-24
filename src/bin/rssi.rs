#![no_std]
#![no_main]

// RSSI measurement across spectrum

use defmt::*;
use {defmt_rtt as _, panic_probe as _};

use embassy_executor::Spawner;
use embassy_stm32::{
    bind_interrupts, exti::ExtiInput, gpio::{Level, Output, Pull, Speed}, mode::Async, peripherals, spi::{Config as SpiConfig, Spi}, time::Hertz, usart::{self, Config as UartConfig, Uart, UartRx, UartTx}
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
const RF_STEP: u32 =      250_000;
const RX_BW  : RxBw =  RxBw::Bw256;
const MEAS_US: u64 = 200;

/// Led modes
static LED_GREEN: SignalLedMode = Signal::new();
static LED_RED: SignalLedMode = Signal::new();

bind_interrupts!(struct UartIrqs {
    USART2 => usart::InterruptHandler<peripherals::USART2>;
});

pub type SignalData = Signal<CriticalSectionRawMutex, (u32,u16)>;
static DATA : SignalData = Signal::new();

pub type SignalCfg = Signal<CriticalSectionRawMutex, (u16,u16,u16)>;
static CFG : SignalCfg = Signal::new();

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
    let (uart_tx,uart_rx) = uart.split();

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
    spawner.spawn(send_to_uart(uart_tx, &DATA)).unwrap();
    spawner.spawn(parse_uart(uart_rx, &CFG)).unwrap();
    let mut rf_min  = RF_MIN;
    let mut rf_max  = RF_MAX;
    let mut rf_step = RF_STEP;
    loop {
        let rssi = lr2021.get_rssi_avg(Duration::from_micros(MEAS_US)).await.expect("RssiAvg");
        // Wait for the UART to be ready
        while DATA.signaled() {
            Timer::after_micros(10).await;
        }
        DATA.signal((rf, rssi));
        // Handle change in configuration
        if let Some((min,max,step)) = CFG.try_take() {
            info!("Config changed to {}:{}:{} !", min, max, step);
            // Min max in MHz
            if (150..1250).contains(&min) {rf_min = min as u32 * 1_000_000;}
            if (150..1250).contains(&max) {rf_max = max as u32 * 1_000_000;}
            // Step in kHz
            if (1..1000).contains(&step) {
                // On Step change ensure we start back at RF MIN
                rf = rf_max;
                rf_step = step as u32 * 1_000;
                let rf_bw = khz_to_bw(step);
                // Change Bandwidth
                lr2021.set_chip_mode(lr2021::system::ChipMode::Fs).await.ok();
                lr2021.set_fsk_modulation(rf_step, PulseShape::Bt0p5, rf_bw, rf_step>>3).await.expect("SetFskModulation");
                lr2021.set_rx(0xFFFFFFFF, true).await.ok();
                info!("[UART] Setting step to {}kHz -> BW = {}", step, rf_bw);
            } else {
                info!("[UART] Range set to {}-{} MHz", min, max);
            }
        }
        // Update current RF
        rf += rf_step;
        if rf > rf_max {
            info!("Wrapping !");
            LED_RED.signal(LedMode::Flash);
            rf = rf_min;
        }
        lr2021.set_rf(rf).await.expect("SetRF");
    }
}

#[embassy_executor::task]
pub async fn send_to_uart(mut uart: UartTx<'static, Async>, signal: &'static SignalData) {
    let mut s: String<32> = String::new();
    loop {
        // Wait for data to send
        let (rf, rssi) = signal.wait().await;
        // Create string "rf : rssi"
        s.clear();
        core::write!(&mut s, "{}:{}\r\n", rf/1000, rssi).ok();
        // Send it on the uart
        uart.write(s.as_bytes()).await.ok();
    }
}

#[embassy_executor::task]
pub async fn parse_uart(mut uart: UartRx<'static, Async>, cfg: &'static SignalCfg) {
    loop {
        // Wait for a command
        let mut buffer = [0u8;32];
        uart.read_until_idle(&mut buffer).await.ok();
        // Parsing: either R[min]-[max] or S[step]
        match buffer[0] {
            b'R' | b'r' => {
                let (min,offset) = parse_num(&buffer[1..]);
                let (max,_) = parse_num(&buffer[1+offset..]);
                cfg.signal((min, max,0));
                info!("[UART] Changing range to : {}MHz to {}MHz", min, max);
            }
            b'S' | b's' => {
                let (step,_) = parse_num(&buffer[1..]);
                cfg.signal((0, 0, step));
            }
            _ => {}
        }
    }
}

fn parse_num(buffer: &[u8]) -> (u16,usize) {
    let mut v = 0u16;
    let mut idx = 0;
    for c in buffer {
        idx += 1;
        match c {
            48..=57 => v = 10*v + (c-48) as u16,
            b'_' => {}
            _ => break,
        }
    }
    (v,idx)
}

fn khz_to_bw(value: u16) -> RxBw {
    match value {
        0..=3=> RxBw::Bw3p5,
        4 => RxBw::Bw4p2,
        5 => RxBw::Bw5p2,
        6 => RxBw::Bw6,
        7 => RxBw::Bw7p4,
        8 => RxBw::Bw8,
        9 => RxBw::Bw9p6,
        10 => RxBw::Bw10,
        11 => RxBw::Bw11,
        12 => RxBw::Bw12,
        13 => RxBw::Bw13,
        14 => RxBw::Bw14,
        15 | 16 => RxBw::Bw16,
        17 => RxBw::Bw17,
        18|19=> RxBw::Bw19,
        20 => RxBw::Bw20,
        21|22=> RxBw::Bw22,
        23 => RxBw::Bw23,
        24 => RxBw::Bw24,
        25..=27 => RxBw::Bw27,
        28..=29 => RxBw::Bw29,
        30..=32=> RxBw::Bw32,
        33=> RxBw::Bw33,
        34=> RxBw::Bw34,
        35=> RxBw::Bw35,
        36..=38=> RxBw::Bw38,
        39..=41=> RxBw::Bw41,
        42..=44=> RxBw::Bw44,
        45|46=> RxBw::Bw46,
        47|48=> RxBw::Bw48,
        49..=55=> RxBw::Bw55,
        56..=59=> RxBw::Bw59,
        60..=64=> RxBw::Bw64,
        65..=66=> RxBw::Bw66,
        67..=69=> RxBw::Bw69,
        70|71=> RxBw::Bw71,
        72..=76=> RxBw::Bw76,
        77..=83=> RxBw::Bw83,
        84..=89=> RxBw::Bw89,
        90..=92=> RxBw::Bw92,
        93..=96=> RxBw::Bw96,
        97..=111=> RxBw::Bw111,
        112..=119=> RxBw::Bw119,
        120..=128=> RxBw::Bw128,
        129..=133=> RxBw::Bw133,
        134..=138=> RxBw::Bw138,
        139..=142=> RxBw::Bw142,
        143..=153=> RxBw::Bw153,
        154..=166=> RxBw::Bw166,
        167..=178=> RxBw::Bw178,
        179..=185=> RxBw::Bw185,
        186..=192=> RxBw::Bw192,
        193..=222=> RxBw::Bw222,
        223..=238=> RxBw::Bw238,
        239..=256=> RxBw::Bw256,
        257..=266=> RxBw::Bw266,
        267..=277=> RxBw::Bw277,
        278..=285=> RxBw::Bw285,
        286..=307=> RxBw::Bw307,
        308..=333=> RxBw::Bw333,
        334..=357=> RxBw::Bw357,
        358..=370=> RxBw::Bw370,
        371..=384=> RxBw::Bw384,
        385..=444=> RxBw::Bw444,
        445..=476=> RxBw::Bw476,
        477..=512=> RxBw::Bw512,
        513..=533=> RxBw::Bw533,
        534..=555=> RxBw::Bw555,
        556..=571=> RxBw::Bw571,
        572..=615=> RxBw::Bw615,
        616..=666=> RxBw::Bw666,
        667..=714=> RxBw::Bw714,
        715..=740=> RxBw::Bw740,
        741..=769=> RxBw::Bw769,
        770..=888=> RxBw::Bw888,
        889..=1111=> RxBw::Bw1111,
        1112..=1333=> RxBw::Bw1333,
        1334..=2222=> RxBw::Bw2222,
        2223..=2666=> RxBw::Bw2666,
        2667..=2857=> RxBw::Bw2857,
        _ => RxBw::Bw3076,
    }
}