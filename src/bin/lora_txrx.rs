#![no_std]
#![no_main]

// LoRa TX/RX demo application
// Blinking led green is for RX, red is for TX
// Long press on user button switch the board role between TX and RX
// Short press either send a packet of incrementing byte or display RX stats in RX

use defmt::*;
use {defmt_rtt as _, panic_probe as _};

use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};

use lr2021_apps::board::{BoardNucleoL476Rg, BoardRole, ButtonPressKind, LedMode, Lr2021Stm32};
use lr2021::{
    lora::{HeaderType, Ldro, LoraBw, LoraCr, Sf},
    radio::{PacketType, RampTime, RxPath},
    status::{Intr, IRQ_MASK_RX_DONE},
    system::ChipMode
};

const PLD_SIZE : u8 = 10;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Starting lora_txrx");

    let board = BoardNucleoL476Rg::init(&spawner).await;
    let mut lr2021 = board.lr2021;
    let mut irq = board.irq;
    BoardNucleoL476Rg::led_green_set(LedMode::BlinkSlow);

    // Packet ID: correspond to first byte sent
    let mut pkt_id = 0_u8;

    // Initialize transceiver for LoRa communication
    // 901MHz, 0dbM, SF5 BW1000, CR 4/5
    lr2021.set_rf(901_000_000).await.expect("Setting RF to 901MHz");
    lr2021.set_rx_path(RxPath::LfPath, 0).await.expect("Setting RX path to LF");
    lr2021.calib_fe(&[]).await.expect("Front-End calibration");

    match lr2021.get_status().await {
        Ok((status, intr)) => info!("Calibration Done: {} | {}", status, intr),
        Err(e) => warn!("Calibration Failed: {}", e),
    }

    lr2021.set_packet_type(PacketType::Lora).await.expect("Setting packet type");
    lr2021.set_lora_modulation(Sf::Sf5, LoraBw::Bw1000, LoraCr::Cr1Ham45Si, Ldro::Off).await.expect("Setting packet type");
    // Packet Preamble 8 Symbols, 10 Byte payload, Explicit header with CRC and up-chirp
    lr2021.set_lora_packet(8, PLD_SIZE, HeaderType::Explicit, true, false).await.expect("Setting packet parameters");
    lr2021.set_tx_params(0, RampTime::Ramp8u).await.expect("Setting TX parameters");

    // Start RX continuous
    match lr2021.set_rx(0xFFFFFFFF, true).await {
        Ok(_) => info!("[RX] Searching Preamble"),
        Err(e) => error!("Fail while set_rx() : {}", e),
    }

    // Set DIO9 as IRQ for RX Done
    lr2021.set_dio_irq(7, Intr::new(IRQ_MASK_RX_DONE)).await.expect("Setting DIO7 as IRQ");

    // Wait for a button press for actions
    let mut button_press = BoardNucleoL476Rg::get_button_evt();

    let mut role = BoardRole::Rx;
    loop {
        match select(button_press.changed(), irq.wait_for_rising_edge()).await {
            Either::First(press) => {
                match (press, role) {
                    // Short press in RX => clear stats
                    (ButtonPressKind::Short, BoardRole::Rx) => show_and_clear_rx_stats(&mut lr2021).await,
                    // Short press in TX => send a packet
                    (ButtonPressKind::Short, BoardRole::Tx) => {
                        send_pkt(&mut lr2021, &mut pkt_id).await;
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

async fn send_pkt(lr2021: &mut Lr2021Stm32, pkt_id: &mut u8) {
    info!("[TX] Sending packet {}", *pkt_id);
    let len = PLD_SIZE as usize;
    // Create payload and send it to the TX FIFO
    for (i,d) in lr2021.buffer_mut().iter_mut().take(len).enumerate() {
        *d = pkt_id.wrapping_add(i as u8);
    }
    lr2021.wr_tx_fifo(len).await.expect("FIFO write");
    lr2021.set_tx(0).await.expect("SetTx");
    *pkt_id += 1;
}

async fn switch_mode(lr2021: &mut Lr2021Stm32, is_rx: bool) {
    lr2021.set_chip_mode(ChipMode::Fs).await.expect("SetFs");
    if is_rx {
        lr2021.set_rx(0xFFFFFFFF, true).await.expect("SetRx");
        BoardNucleoL476Rg::led_red_set(LedMode::Off);
        BoardNucleoL476Rg::led_green_set(LedMode::BlinkSlow);
        info!(" -> Switched to RX");
    } else {
        lr2021.set_lora_packet(8, PLD_SIZE, HeaderType::Explicit, true, false).await.expect("Setting packet parameters");
        BoardNucleoL476Rg::led_red_set(LedMode::BlinkSlow);
        BoardNucleoL476Rg::led_green_set(LedMode::Off);
        info!(" -> Switching to FS: ready for TX");
    }
}

async fn show_rx_pkt(lr2021: &mut Lr2021Stm32) {
    let pkt_len = lr2021.get_rx_pkt_len().await.expect("RX Fifo level");
    let nb_byte = pkt_len.min(16) as usize; // Make sure to not read more than the local buffer size
    lr2021.rd_rx_fifo(nb_byte).await.expect("RX FIFO Read");
    let intr = lr2021.get_and_clear_irq().await.expect("Getting intr");
    let status = lr2021.get_lora_packet_status_adv().await.expect("RX status");
    let snr = status.snr_pkt();
    let snr_frac = (snr&3) * 25;
    info!("[RX] Payload = {:02x} | intr={:08x} | RSSI=-{}dBm, SNR={}.{:02}, FEI={}",
        lr2021.buffer()[..nb_byte],
        intr.value(),
        status.rssi_pkt()>>1,
        snr>>2, snr_frac,
        status.freq_offset()
    );
}
