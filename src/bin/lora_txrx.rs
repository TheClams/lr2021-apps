#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU8, Ordering};

use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_stm32::spi::{Config, Spi};
use embassy_stm32::{
    exti::ExtiInput,
    gpio::{Input, Level, Output, Pull, Speed},
    time::Hertz,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, watch::Watch};
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

use lr2021_apps::lr2021::Lr2021;

/// Global variable to store the board state
static BOARD_ROLE: AtomicU8 = AtomicU8::new(0);
/// Generate event when the button is press with short (0) or long (1) duration
static BUTTON_PRESS: Watch<CriticalSectionRawMutex, u8, 3> = Watch::new();

/// Board Role: TX or RX
#[derive(Debug, Clone, Copy, Format, PartialEq)]
enum BoardRole {
    Rx = 0,
    Tx = 1,
}
impl BoardRole {
    pub fn toggle(&mut self) {
        *self = match self {
            BoardRole::Rx => BoardRole::Tx,
            BoardRole::Tx => BoardRole::Rx,
        }
    }
}

impl From<u8> for BoardRole {
    fn from(value: u8) -> Self {
        match value {
            1 => BoardRole::Tx,
            _ => BoardRole::Rx,
        }
    }
}

/// Task to blink a led
#[embassy_executor::task(pool_size = 2)]
async fn blink(mut led: Output<'static>, led_role: BoardRole) {
    let mut button_press = BUTTON_PRESS.receiver().unwrap();
    loop {
        let board_role: BoardRole = BOARD_ROLE.load(Ordering::Relaxed).into();
        if board_role == led_role {
            led.toggle();
            Timer::after_millis(100).await;
        } else {
            led.set_low();
            button_press.changed().await;
        }
    }
}

/// Task to handle the user interface:
///   - a long press change the board role (TX or RX)
///   - a short press either send a packet (TX mode) or clear the RX stat (RX mode)
#[embassy_executor::task]
async fn user_intf(mut button: ExtiInput<'static>) {
    let mut role = BoardRole::Rx;
    let button_press = BUTTON_PRESS.sender();
    loop {
        button.wait_for_falling_edge().await;
        // Small wait to debounce button press
        Timer::after_millis(5).await;
        // Determine if this is a short or long press
        match select(button.wait_for_high(), Timer::after_millis(500)).await {
            // Short press
            Either::First(_) => button_press.send(0),
            // Long press
            Either::Second(_) => {
                role.toggle();
                BOARD_ROLE.store(role as u8, Ordering::Relaxed);
                button_press.send(1);
            }
        }
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());
    info!("Starting lora_txrx");

    // Start tasks to blink the TX/RX leds
    // and handle the button press to
    let led_tx = Output::new(p.PC1, Level::High, Speed::Low);
    spawner.spawn(blink(led_tx, BoardRole::Tx)).unwrap();

    let led_rx = Output::new(p.PC0, Level::High, Speed::Low);
    spawner.spawn(blink(led_rx, BoardRole::Rx)).unwrap();

    let button = ExtiInput::new(p.PC13, p.EXTI13, Pull::Up);
    spawner.spawn(user_intf(button)).unwrap();

    // Control pins
    let busy = Input::new(p.PB3, Pull::Up);
    let nreset = Output::new(p.PA0, Level::High, Speed::Low);
    let irq = ExtiInput::new(p.PA10, p.EXTI10, Pull::Up);

    // SPI
    let mut spi_config = Config::default();
    spi_config.frequency = Hertz(4_000_000);
    let spi = Spi::new(
        p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA1_CH3, p.DMA1_CH2, spi_config,
    );
    let nss = Output::new(p.PA8, Level::High, Speed::VeryHigh);

    // Create driver and reset board
    let mut lr2021 = Lr2021::new(nreset, busy, irq, spi, nss);
    lr2021.reset().await.expect("Resetting chip !");

    // Check version
    let version = lr2021
        .get_version()
        .await
        .expect("Reading firmware version !");
    info!("FW Version {}", version);

    // Packet ID: correspond to first byte sent
    let mut pkt_id = 0_u8;

    // Initialized transceiver for LoRa communication

    // Wait for a button press for actions
    let mut button_press = BUTTON_PRESS.receiver().unwrap();
    loop {
        let press = button_press.changed().await;
        let role: BoardRole = BOARD_ROLE.load(Ordering::Relaxed).into();
        match (press, role) {
            // Short press in RX => clear stats
            (0, BoardRole::Rx) => {
                info!("[RX] Clearing stats");
            }
            // Short press in TX => send a packet
            (0, BoardRole::Tx) => {
                info!("[TX] Sending packet {}", pkt_id);
                pkt_id += 1;
            }
            // Long press: switch role TX/RX
            (1, BoardRole::Rx) => {
                info!(" -> Switching to RX");
            }
            (1, BoardRole::Tx) => {
                info!(" -> Switching to TX");
            }
            (n, _) => warn!("Button press with value {} not implemented !", n),
        }
    }
}
