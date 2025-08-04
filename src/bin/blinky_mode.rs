#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU8, Ordering};

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{exti::ExtiInput, gpio::{Level, Output, Pull, Speed}};
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

/// Global variable handling Blinking speed
static BLINK_MODE: AtomicU8 = AtomicU8::new(0);

/// Blinking mode
#[derive(Format)]
enum BlinkMode {
    Fast = 0, Medium = 1, Slow = 2
}

impl BlinkMode {
    /// Return the delay in ms associated with a blinking mode
    fn delay_ms(&self) -> u64 {
        match self {
            BlinkMode::Fast   => 100,
            BlinkMode::Medium => 500,
            BlinkMode::Slow   => 2500,
        }
    }

    /// Return the delay in ms associated with a blinking mode
    fn next(&mut self) {
        *self = match self {
            BlinkMode::Fast   => BlinkMode::Medium,
            BlinkMode::Medium => BlinkMode::Slow,
            BlinkMode::Slow   => BlinkMode::Fast,
        }
    }

    /// Return the delay in ms associated with a blinking mode
    fn to_u8(&self) -> u8 {
        match self {
            BlinkMode::Fast   => 0,
            BlinkMode::Medium => 1,
            BlinkMode::Slow   => 2,
        }
    }
}

impl From<u8> for BlinkMode {
    fn from(value: u8) -> Self {
        match value {
            2 => BlinkMode::Slow,
            1 => BlinkMode::Medium,
            _ => BlinkMode::Fast
        }
    }
}

/// Task to blink a led
#[embassy_executor::task]
async fn blink(mut led: Output<'static>) {

    loop {
        let mode : BlinkMode = BLINK_MODE.load(Ordering::Relaxed).into();
        let delay = mode.delay_ms();
        led.set_high();
        Timer::after_millis(delay).await;
        led.set_low();
        Timer::after_millis(delay).await;
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());
    info!("Hello from blinky_mode");

    // Spawn a blink task
    let led = Output::new(p.PA5, Level::High, Speed::Low);
    spawner.spawn(blink(led)).unwrap();

    // Get an interrupt on the button pin to wait
    let mut button = ExtiInput::new(p.PC13, p.EXTI13, Pull::Up);
    let mut mode = BlinkMode::Fast;

    loop {
        button.wait_for_low().await;
        mode.next();
        info!("Button pressed => {}", mode);
        BLINK_MODE.store(mode.to_u8(), Ordering::Relaxed);
        button.wait_for_high().await;
    }

}
