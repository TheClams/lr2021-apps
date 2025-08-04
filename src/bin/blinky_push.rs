#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Input, Level, Output, Pull, Speed};
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());
    info!("Hello from blinky_push");

    let mut led = Output::new(p.PA5, Level::High, Speed::Low);
    let button = Input::new(p.PC13, Pull::Up);

    loop {
        let wait = if button.is_low() {1000} else {250};
        led.set_high();
        Timer::after_millis(wait).await;
        led.set_low();
        Timer::after_millis(wait).await;
    }

}
