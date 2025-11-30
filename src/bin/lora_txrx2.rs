#![no_std]
#![no_main]

//! # LoRa TX/RX through lora-phy wrapper demo application
//!
//! Blinking led green is for RX, red is for TX
//! Long press on user button switch the board role between TX and RX
//! Short press either send a packet of incrementing byte or display RX stats in RX

use defmt::*;
use embassy_futures::select::{select, Either};
use embassy_stm32::{
    exti::ExtiInput,
    gpio::{Level, Output, Pull, Speed},
    spi::{Config as SpiConfig, Spi},
    time::Hertz,
};

use {defmt_rtt as _, panic_probe as _};

use lr2021::{BusyAsync, system::DioNum};
use lr2021_apps::board::*;
use lr2021_loraphy::{Bandwidth, CodingRate, IrqState, Lr2021LoraPhy, PacketParams, RadioKind, RadioMode, SpreadingFactor};

const PLD_SIZE : u8 = 10;

type Lr2021Stm32 = Lr2021LoraPhy<Output<'static>,SpiWrapper, ExtiInput<'static>, BusyAsync<ExtiInput<'static>>>;

#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) {
    info!("Starting lora_txrx2 (loraphy)");

    // Init STM32 peripherals
    let p = stm32_init();

    // Leds & buttons
    let led_red = Output::new(p.PC1, Level::High, Speed::Low);
    let led_green = Output::new(p.PC0, Level::High, Speed::Low);
    let button = ExtiInput::new(p.PC13, p.EXTI13, Pull::Up);

    // Start the tasks
    spawner.spawn(blink(led_red, &LED_RED_MODE)).unwrap();
    spawner.spawn(blink(led_green, &LED_GREEN_MODE)).unwrap();
    spawner.spawn(user_intf(button, &BUTTON_PRESS)).unwrap();
    BoardNucleoL476Rg::led_red_set(LedMode::Off);
    BoardNucleoL476Rg::led_green_set(LedMode::BlinkSlow);

    // Aquire pins needed by the LR2021 Driver
    let busy = ExtiInput::new(p.PB3, p.EXTI3, Pull::Up);
    let nreset = Output::new(p.PA0, Level::High, Speed::Low);
    let irq = ExtiInput::new(p.PB0, p.EXTI0, Pull::None); // DIO7
    let nss = Output::new(p.PA8, Level::High, Speed::VeryHigh);
    // Get SPI device
    let mut spi_config = SpiConfig::default();
    spi_config.frequency = Hertz(12_000_000);
    let spi = SpiWrapper(Spi::new_blocking(p.SPI1, p.PA5, p.PA7, p.PA6, spi_config));

    // Create the LR2021 lora-phy wrapper
    let mut lr2021 = Lr2021LoraPhy::new(nreset, busy, spi, nss, irq, DioNum::Dio7);

    let modulation = lr2021.create_modulation_params(
            SpreadingFactor::_5,
            Bandwidth::_500KHz,
            CodingRate::_4_5,
            901_000_000,
        ).expect("Creating Modulation Params");

    let pkt_params = lr2021.create_packet_params(8, false, PLD_SIZE, true, false, &modulation)
        .expect("Creating Modulation Params");

    lr2021.set_channel(modulation.frequency_in_hz).await.expect("set_channel");
    // lr2021.calibrate_image(modulation.frequency_in_hz).await.expect("calibrate_image");
    // match lr2021.driver.get_status().await {
    //     Ok((status, intr)) => info!("Calibration Done: {} | {}", status, intr),
    //     Err(e) => warn!("Calibration Failed: {}", e),
    // }

    lr2021.set_modulation_params(&modulation).await.expect("set_modulation_params");
    lr2021.set_packet_params(&pkt_params).await.expect("set_packet_params");
    lr2021.set_irq_params(Some(RadioMode::Standby)).await.expect("set_irq_params");

    // Wait for a button press for actions
    let mut button_press = BoardNucleoL476Rg::get_button_evt();

    // Packet ID: correspond to first byte sent
    let mut pkt_id = 0_u8;

    let mut buffer : [u8; 256] = [0; 256];

    // // Initialize transceiver for LoRa communication
    // // 901MHz, 0dbM, SF5 BW1000, CR 4/5
    // lr2021.set_rx_path(RxPath::LfPath, RxBoost::Off).await.expect("Setting RX path to LF");

    let mut role = BoardRole::Rx;
    loop {
        match select(button_press.changed(), lr2021.await_irq()).await {
            Either::First(press) => {
                match (press, role) {
                    // Short press in RX => clear stats
                    (ButtonPressKind::Short, BoardRole::Rx) => {
                        show_and_clear_rx_stats(&mut lr2021).await;
                    }
                    // Short press in TX => send a packet
                    (ButtonPressKind::Short, BoardRole::Tx) => {
                        send_pkt(&mut lr2021, &mut pkt_id, &mut buffer).await;
                        BoardNucleoL476Rg::led_red_set(LedMode::Flash);
                    }
                    // Long press: switch role TX/RX
                    (ButtonPressKind::Long, _) => {
                        role.toggle();
                        switch_mode(&mut lr2021, role.is_rx()).await;
                    }
                    (n, r) => warn!("{} in role {} not implemented !", n, r),
                }
            }
            // RX Interrupt
            Either::Second(_) => {
                let radio_mode = if role.is_rx() {RadioMode::Receive(lr2021_loraphy::RxMode::Continuous)} else {RadioMode::Transmit};
                if let Some(intr) = lr2021.get_irq_state(radio_mode, None).await.expect("GetIrqState") {
                    if intr == IrqState::Done {
                        BoardNucleoL476Rg::led_green_set(LedMode::Flash);
                        if role.is_rx() {
                            show_rx_pkt(&mut lr2021, &mut buffer, &pkt_params).await;
                        }
                    }
                }
                lr2021.clear_irq_status().await.expect("clear_irq_status");
            }
        }
    }
}

async fn show_and_clear_rx_stats(lr2021: &mut Lr2021Stm32) {
    let stats = lr2021.driver.get_lora_rx_stats().await.expect("RX stats");
    info!("[RX] Clearing stats | RX={}, CRC Err={}, HdrErr={}, FalseSync={}",
        stats.pkt_rx(),
        stats.crc_error(),
        stats.header_error(),
        stats.false_sync(),
    );
}

async fn send_pkt(lr2021: &mut Lr2021Stm32, pkt_id: &mut u8, buffer: &mut [u8]) {
    info!("[TX] Sending packet {}", *pkt_id);
    let len = PLD_SIZE as usize;
    for (i,d) in buffer.iter_mut().take(len).enumerate() {
        *d = pkt_id.wrapping_add(i as u8);
    }
    lr2021.set_payload(&buffer[..len]).await.expect("set_payload");
    lr2021.do_tx().await.expect("do_tx");
    *pkt_id += 1;
}

async fn switch_mode(lr2021: &mut Lr2021Stm32, is_rx: bool) {
    lr2021.set_standby().await.expect("set_standby");
    if is_rx {
        lr2021.do_rx(lr2021_loraphy::RxMode::Continuous).await.expect("SetRx");
        BoardNucleoL476Rg::led_red_set(LedMode::Off);
        BoardNucleoL476Rg::led_green_set(LedMode::BlinkSlow);
        info!(" -> Switched to RX");
    } else {
        BoardNucleoL476Rg::led_red_set(LedMode::BlinkSlow);
        BoardNucleoL476Rg::led_green_set(LedMode::Off);
        info!(" -> Switching to FS: ready for TX");
    }
}

async fn show_rx_pkt(lr2021: &mut Lr2021Stm32, buffer: &mut [u8], pkt_params: &PacketParams) {
    let nb_byte = lr2021.get_rx_payload(pkt_params, buffer).await.expect("RX FIFO Read") as usize;
    let status = lr2021.get_rx_packet_status().await.expect("RX status");
    info!("[RX] Payload = {:02x} | RSSI={}dBm, SNR={}dB",
        buffer[..nb_byte],
        status.rssi,
        status.snr,
    );
}
