#![no_std]
#![no_main]

//! FLRC TX/RX demo application
//! Blinking led green is for RX, red is for TX
//! Long press on user button switch the board role between TX and RX
//! Short press either send a packet of incrementing byte or display RX stats in RX
//! Double press in TX changes the syncword used

use defmt::*;
use {defmt_rtt as _, panic_probe as _};

use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};

use lr2021_apps::board::{BoardNucleoL476Rg, BoardRole, ButtonPressKind, LedMode, Lr2021Stm32};
use lr2021::{
    flrc::*,
    radio::{FallbackMode, PaLfMode, PacketType, RampTime, RxBoost, RxPath},
    status::{Intr, IRQ_MASK_RX_DONE, IRQ_MASK_TX_DONE},
    system::{ChipMode, DioNum}, PulseShape
};

const PLD_SIZE : u16 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Format)]
pub enum SwSel {Sw1,Sw2,Sw3}
impl SwSel {
    /// Return syncword value
    pub fn value(&self) -> u32 {
        match self {
            SwSel::Sw1 => 0xCD05CAFE,
            SwSel::Sw2 => 0x12345678,
            SwSel::Sw3 => 0x9ABCDEF0,
        }
    }
    /// Return syncword value
    pub fn sw_tx(&self) -> SwTx {
        match self {
            SwSel::Sw1 => SwTx::Sw1,
            SwSel::Sw2 => SwTx::Sw2,
            SwSel::Sw3 => SwTx::Sw3,
        }
    }
    /// Swicth to next Syncword
    pub fn next(&mut self) {
        *self = match self {
            SwSel::Sw1 => SwSel::Sw2,
            SwSel::Sw2 => SwSel::Sw3,
            SwSel::Sw3 => SwSel::Sw1,
        }
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Starting flrc_txrx");

    let board = BoardNucleoL476Rg::init(&spawner).await;
    let mut lr2021 = board.lr2021;
    let mut irq = board.irq;

    // Packet ID: correspond to first byte sent
    let mut pkt_id = 0_u8;
    let mut sw_sel = SwSel::Sw1;

    // Initialize transceiver for LoRa communication
    lr2021.set_rf(900_000_000).await.expect("Setting RF to 900MHz");
    lr2021.set_rx_path(RxPath::LfPath, RxBoost::Off).await.expect("Setting RX path to LF");
    // lr2021.set_rf(2_400_000_000).await.expect("Setting RF to 2.4GHz");
    // lr2021.set_rx_path(RxPath::HfPath, 0).await.expect("Setting RX path to HF");
    lr2021.calib_fe(&[]).await.expect("Front-End calibration");
    match lr2021.get_status().await {
        Ok((status, intr)) => info!("Calibration Done: {} | {}", status, intr),
        Err(e) => warn!("Calibration Failed: {}", e),
    }

    // lr2021.set_pa_hf().await.expect("Set PA HF");
    lr2021.set_pa_lf(PaLfMode::LfPaFsm, 6, 7).await.expect("Set PA HF");
    lr2021.set_tx_params(0, RampTime::Ramp16u).await.expect("Setting TX parameters");

    // Configure FLRC
    lr2021.set_packet_type(PacketType::Flrc).await.expect("Setting packet type");
    lr2021.set_flrc_modulation(FlrcBitrate::Br2600, FlrcCr::None, PulseShape::Bt1p0).await.expect("Setting packet type");
    lr2021.set_flrc_syncword(1, 0xCD05CAFE, true).await.expect("SetSw1");
    lr2021.set_flrc_syncword(2, 0x12345678, true).await.expect("SetSw2");
    lr2021.set_flrc_syncword(3, 0x9ABCDEF0, true).await.expect("SetSw3");
    // Packet with 16b preamble, 32b syncword, using Syncword1, dynamic length with CRC on 24b
    let mut flrc_params = FlrcPacketParams::new(AgcPblLen::Len16Bits, SwLen::Sw32b, SwTx::Sw1, SwMatch::Match123, PktFormat::Dynamic, Crc::Crc24, PLD_SIZE);
    lr2021.set_flrc_packet(&flrc_params).await.expect("SetPacket");
    lr2021.set_fallback(FallbackMode::Fs).await.expect("Set fallback");

    // Start RX continuous
    lr2021.set_rx(0xFFFFFFFF, true).await.expect("SetRX");
    BoardNucleoL476Rg::led_green_set(LedMode::BlinkSlow);

    // Set DIO7 as IRQ for RX Done
    lr2021.set_dio_irq(DioNum::Dio7, Intr::new(IRQ_MASK_TX_DONE|IRQ_MASK_RX_DONE)).await.expect("Setting DIO7 as IRQ");

    // Create data buffer to test the wr_fifo_from and rf_fifo_to APIs
    let mut data = [0;16];

    let mut role = BoardRole::Rx;

    // Wait for a button press for actions
    let mut button_press = BoardNucleoL476Rg::get_button_evt();
    loop {
        match select(button_press.changed(), irq.wait_for_rising_edge()).await {
            Either::First(press) => {
                match (press, role) {
                    // Short press in RX => clear stats
                    (ButtonPressKind::Short, BoardRole::Rx) => show_and_clear_rx_stats(&mut lr2021).await,
                    // Short press in TX => send a packet
                    (ButtonPressKind::Short, BoardRole::Tx) => send_pkt(&mut lr2021, &mut pkt_id, &mut data).await,
                    // Double press in TX => Change Syncword
                    (ButtonPressKind::Double, BoardRole::Tx) => {
                        sw_sel.next();
                        flrc_params.sw_tx = sw_sel.sw_tx();
                        lr2021.set_flrc_packet(&flrc_params)
                            .await.expect("Setting packet parameters");
                        info!("Switching to {}", sw_sel);
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
                let intr = lr2021.get_and_clear_irq().await.expect("Getting intr");
                if intr.tx_done() {
                    BoardNucleoL476Rg::led_red_set(LedMode::Flash);
                }
                if /*lvl > 0 && */intr.rx_done() {
                    show_rx_pkt(&mut lr2021, &mut data, intr).await;
                    if !intr.crc_error() {
                        BoardNucleoL476Rg::led_green_set(LedMode::Flash);
                    }
                }
            }
        }
    }
}

async fn show_and_clear_rx_stats(lr2021: &mut Lr2021Stm32) {
    let stats = lr2021.get_flrc_rx_stats_adv().await.expect("RX stats");
    info!("[RX] Clearing stats | RX={}, CRC Err={}, LenErr={}, FalseSync={}",
        stats.pkt_rx(),
        stats.crc_error(),
        stats.len_error(),
        stats.false_sync(),
    );
    lr2021.clear_rx_stats().await.unwrap();
}

async fn send_pkt(lr2021: &mut Lr2021Stm32, pkt_id: &mut u8, data: &mut [u8]) {
    info!("[TX] Sending packet {}", *pkt_id);
    // Create payload and send it to the TX FIFO
    for (i,d) in data.iter_mut().take(PLD_SIZE.into()).enumerate() {
        *d = pkt_id.wrapping_add(i as u8);
    }
    lr2021.wr_tx_fifo_from(&data[..PLD_SIZE.into()]).await.expect("FIFO write");
    lr2021.set_tx(0).await.expect("SetTx");
    *pkt_id += 1;
}

async fn switch_mode(lr2021: &mut Lr2021Stm32, is_rx: bool) {
    lr2021.set_chip_mode(ChipMode::Fs).await.expect("SetFs");
    if is_rx {
        lr2021.set_rx(0xFFFFFFFF, true).await.expect("SetRx");
        info!(" -> Switched to RX");
        BoardNucleoL476Rg::led_red_set(LedMode::Off);
        BoardNucleoL476Rg::led_green_set(LedMode::BlinkSlow);
    } else {
        info!(" -> Switching to FS: ready for TX");
        BoardNucleoL476Rg::led_red_set(LedMode::BlinkSlow);
        BoardNucleoL476Rg::led_green_set(LedMode::Off);
    }
}

async fn show_rx_pkt(lr2021: &mut Lr2021Stm32, data: &mut [u8], intr: Intr) {
    let status = lr2021.get_flrc_packet_status().await.expect("RX status");
    let nb_byte = status.pkt_len().min(16) as usize; // Make sure to not read more than the local buffer size
    lr2021.rd_rx_fifo_to(&mut data[..nb_byte]).await.expect("RX FIFO Read");

    info!("[RX] Payload = {:02x} ({}) SW{} | intr={:08x} -> {} | RSSI=-{}dBm",
        data[..nb_byte],
        status.pkt_len(),
        status.sw_num(),
        intr.value(),
        intr,
        status.rssi_avg()>>1,
    );
}
