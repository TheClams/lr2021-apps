#![no_std]
#![no_main]

//! # LoRa TX/RX demo application
//!
//! Blinking led green is for RX, red is for TX
//! Long press on user button switch the board role between TX and RX
//! Short press either send a packet of incrementing byte or display RX stats in RX

use defmt::*;
use {defmt_rtt as _, panic_probe as _};

use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};

use lr2021_apps::board::{BoardNucleoL476Rg, ButtonPressKind, LedMode, Lr2021Stm32};
use lr2021::{
    lora::{Ldro, LoraBw, LoraModulationParams, LoraPacketParams, Sf, SidedetCfg},
    radio::{PacketType, RxBoost, RxPath},
    status::{IRQ_MASK_RX_DONE, Intr}, system::{ChipMode, DioNum}
};

const PLD_SIZE : u8 = 10;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Starting lora_txrx");

    let board = BoardNucleoL476Rg::init(&spawner).await;
    let mut lr2021 = board.lr2021;
    let mut irq = board.irq;
    BoardNucleoL476Rg::led_green_set(LedMode::BlinkSlow);

    // Initialize transceiver for LoRa communication
    // 901MHz, 0dbM, SF5 BW1000, CR 4/5
    lr2021.set_rf(868_100_000).await.expect("Setting RF to 901MHz");
    lr2021.set_rx_path(RxPath::LfPath, RxBoost::Off).await.expect("Setting RX path to LF");
    lr2021.calib_fe(&[]).await.expect("Front-End calibration");

    match lr2021.get_status().await {
        Ok((status, intr)) => info!("Calibration Done: {} | {}", status, intr),
        Err(e) => warn!("Calibration Failed: {}", e),
    }
    // Create two set of modulation configuration to toggle between
    let mod_sf12 = LoraModulationParams::basic(Sf::Sf12, LoraBw::Bw125);
    let sd_cfg_9_11 = [
        SidedetCfg::new(Sf::Sf11, Ldro::On , false),
        SidedetCfg::new(Sf::Sf10, Ldro::Off, false),
        // SidedetCfg::new(Sf::Sf9 , Ldro::Off, false),
    ];
    let mod_sf10 = LoraModulationParams::basic(Sf::Sf10, LoraBw::Bw125);
    let sd_cfg_7_9 = [
        SidedetCfg::new(Sf::Sf9, Ldro::Off, false),
        SidedetCfg::new(Sf::Sf8, Ldro::Off, false),
        SidedetCfg::new(Sf::Sf7, Ldro::Off, false),
    ];

    let mut high_sf = false;
    let packet_params = LoraPacketParams::basic(PLD_SIZE, &mod_sf12);

    lr2021.set_packet_type(PacketType::Lora).await.expect("Setting packet type");
    lr2021.set_lora_modulation(&mod_sf12).await.expect("Setting packet type");
    lr2021.set_lora_sidedet_cfg(&sd_cfg_9_11).await.expect("Setting SideDetector");
    // lr2021.set_lora_sidedet_cfg(&sd_cfg_7_9).await.expect("Setting SideDetector");
    // Packet Preamble 8 Symbols, 10 Byte payload, Explicit header with CRC and up-chirp
    lr2021.set_lora_packet(&packet_params).await.expect("Setting packet parameters");
    BoardNucleoL476Rg::led_red_set(LedMode::Off);
    BoardNucleoL476Rg::led_green_set(LedMode::BlinkSlow);
    info!(" -> Switched to RX");

    // Start RX continuous
    match lr2021.set_rx(0xFFFFFFFF, true).await {
        Ok(_) => info!("[RX] Searching Preamble"),
        Err(e) => error!("Fail while set_rx() : {}", e),
    }

    // Set DIO9 as IRQ for RX Done
    lr2021.set_dio_irq(DioNum::Dio7, Intr::new(IRQ_MASK_RX_DONE)).await.expect("Setting DIO7 as IRQ");

    // Wait for a button press for actions
    let mut button_press = BoardNucleoL476Rg::get_button_evt();

    loop {
        match select(button_press.changed(), irq.wait_for_rising_edge()).await {
            Either::First(press) => {
                match press {
                    // Short press in RX => clear stats
                    ButtonPressKind::Short => show_and_clear_rx_stats(&mut lr2021).await,
                    // Long press: switch role TX/RX
                    ButtonPressKind::Long => {
                        high_sf = !high_sf;
                        lr2021.set_chip_mode(ChipMode::Fs).await.expect("SetFs");
                        if high_sf {
                            lr2021.set_lora_sidedet_cfg(&sd_cfg_9_11).await.expect("Setting SideDetector");
                            lr2021.set_lora_modulation(&mod_sf12).await.expect("Setting packet type");
                            info!("Monitoring SF 9-12 ...");
                        } else {
                            lr2021.set_lora_sidedet_cfg(&sd_cfg_7_9).await.expect("Setting SideDetector");
                            lr2021.set_lora_modulation(&mod_sf10).await.expect("Setting packet type");
                            info!("Monitoring SF 7-10 ...");
                        }
                        lr2021.set_rx(0xFFFFFFFF, true).await.expect("SetRx");
                    }
                    n => warn!("{} not implemented !", n),
                }
            }
            // RX Interrupt
            Either::Second(_) => {
                BoardNucleoL476Rg::led_green_set(LedMode::Flash);
                show_rx_pkt(&mut lr2021).await;
            }
        }
    }
}

async fn show_and_clear_rx_stats(lr2021: &mut Lr2021Stm32) {
    let stats = lr2021.get_lora_rx_stats().await.expect("RX stats");
    info!("[RX] Clearing stats | RX={}, CRC Err={}, HdrErr={}, FalseSync={}",
        stats.pkt_rx(),
        stats.crc_error(),
        stats.header_error(),
        stats.false_sync(),
    );
}

async fn show_rx_pkt(lr2021: &mut Lr2021Stm32) {
    let pkt_len = lr2021.get_rx_pkt_len().await.expect("RX Fifo level");
    let nb_byte = pkt_len.min(16) as usize; // Make sure to not read more than the local buffer size
    lr2021.rd_rx_fifo(nb_byte).await.expect("RX FIFO Read");
    let intr = lr2021.get_and_clear_irq().await.expect("Getting intr");
    let status = lr2021.get_lora_packet_status().await.expect("RX status");
    let snr = status.snr_pkt();
    let snr_frac = (snr&3) * 25;
    info!("[RX] Payload = {:02x} | intr={:08x} | RSSI=-{}dBm, SNR={}.{:02}",
        lr2021.buffer()[..nb_byte],
        intr.value(),
        status.rssi_pkt()>>1,
        snr>>2, snr_frac,
    );
}
