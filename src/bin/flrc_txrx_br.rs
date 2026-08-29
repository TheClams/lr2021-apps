#![no_std]
#![no_main]

//! FLRC TX/RX demo application
//! Blinking led green is for RX, red is for TX
//! Long press on user button switch the board role between TX and RX
//! Short press either send a packet of incrementing byte or display RX stats in RX
//! Double press in TX changes the syncword used

use defmt::*;
use embassy_stm32::usart::Uart;
use {defmt_rtt as _, panic_probe as _};

use embassy_time::Timer;
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};

use core::fmt::Write;
use heapless::String;

use lr2021_apps::board::{BoardNucleoL476Rg, BoardRole, ButtonPressKind, LedMode, Lr2021Stm32};
use lr2021::{
    flrc::*,
    radio::{FallbackMode, PaLfMode, PacketType, RampTime, RxBoost, RxPath},
    status::{Intr, IRQ_MASK_RX_DONE, IRQ_MASK_TX_DONE},
    system::{ChipMode, DioNum}, PulseShape
};

const PLD_SIZE : u16 = 32;


#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Starting flrc_txrx");

    let board = BoardNucleoL476Rg::init(&spawner).await;
    let mut lr2021 = board.lr2021;
    let mut irq = board.irq;
    let mut uart = board.uart;

    // Packet ID: correspond to first byte sent
    let mut pkt_id = 0_u8;
    let mut br_sel = FlrcBitrate::Br0260;

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
    lr2021.set_flrc_modulation(br_sel, FlrcCr::Cr23, PulseShape::Bt1p0).await.expect("Setting packet type");
    lr2021.set_flrc_syncword(1, 0xCD05CAFE, true).await.expect("SetSw1");
    // Packet with 16b preamble, 32b syncword, using Syncword1, dynamic length with CRC on 24b
    let flrc_params = FlrcPacketParams::new(AgcPblLen::Len16Bits, SwLen::Sw32b, SwTx::Sw1, SwMatch::Match1, PktFormat::Dynamic, Crc::Crc24, PLD_SIZE);
    lr2021.set_flrc_packet(&flrc_params).await.expect("SetPacket");
    lr2021.set_fallback(FallbackMode::Fs).await.expect("Set fallback");

    // Start RX continuous
    // lr2021.set_rx(0xFFFFFFFF, true).await.expect("SetRX");
    // BoardNucleoL476Rg::led_green_set(LedMode::BlinkSlow);

    // Set DIO7 as IRQ for RX Done
    lr2021.set_dio_irq(DioNum::Dio7, Intr::new(IRQ_MASK_TX_DONE|IRQ_MASK_RX_DONE)).await.expect("Setting DIO7 as IRQ");

    // Create data buffer to test the wr_fifo_from and rf_fifo_to APIs
    let mut data = [0;PLD_SIZE as usize+8];

    let mut role = BoardRole::Rx;
    let mut active = false;

    // Wait for a button press for actions
    let mut button_press = BoardNucleoL476Rg::get_button_evt();
    loop {
        match select(button_press.changed(), irq.wait_for_rising_edge()).await {
            Either::First(press) => {
                match (press, role) {
                    // Short press in RX => clear stats
                    (ButtonPressKind::Short, BoardRole::Rx) => {
                        if active {
                            info!("Stopping RX");
                            show_and_clear_rx_stats(&mut lr2021).await;
                            BoardNucleoL476Rg::led_green_set(LedMode::Off);
                            lr2021.set_chip_mode(ChipMode::Fs).await.expect("SetFs");
                            uart.write("DONE\r\n".as_bytes()).await.ok();
                        } else {
                            info!("Starting RX");
                            lr2021.set_rx(0xFFFFFFFF, true).await.expect("SetRX");
                            BoardNucleoL476Rg::led_green_set(LedMode::BlinkSlow);
                        }
                        active = !active;
                    }
                    // Short press in TX => send a packet
                    (ButtonPressKind::Short, BoardRole::Tx) => {
                        if active {
                            info!("Stopping TX");
                        } else {
                            info!("Starting TX");
                            send_pkt(&mut lr2021, &mut pkt_id, &mut data).await;
                        }
                        active = !active;
                    },
                    // Double press in TX => Change Syncword
                    (ButtonPressKind::Double, _) => {
                        br_sel = match br_sel {
                            FlrcBitrate::Br2600 => FlrcBitrate::Br0260,
                            FlrcBitrate::Br2080 => FlrcBitrate::Br2600,
                            FlrcBitrate::Br1300 => FlrcBitrate::Br2080,
                            FlrcBitrate::Br1040 => FlrcBitrate::Br1300,
                            FlrcBitrate::Br0650 => FlrcBitrate::Br1040,
                            FlrcBitrate::Br0520 => FlrcBitrate::Br0650,
                            FlrcBitrate::Br0325 => FlrcBitrate::Br0520,
                            FlrcBitrate::Br0260 => FlrcBitrate::Br0325,
                        };
                        lr2021.set_flrc_modulation(br_sel, FlrcCr::Cr23, PulseShape::Bt1p0).await.expect("Setting packet type");
                        info!("Switching to {}", br_sel);
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
                    // Send one packet every 100ms when TX is enabled
                    if active {
                        Timer::after_millis(100).await;
                        send_pkt(&mut lr2021, &mut pkt_id, &mut data).await;
                    }
                }
                else if intr.rx_done() {
                    show_rx_pkt(&mut lr2021, &mut data, intr, &mut uart).await;
                    if !intr.crc_error() {
                        BoardNucleoL476Rg::led_green_set(LedMode::Flash);
                    }
                }
            }
        }
    }
}

async fn show_and_clear_rx_stats(lr2021: &mut Lr2021Stm32) {
    let stats = lr2021.get_flrc_rx_stats().await.expect("RX stats");
    info!("[RX] Clearing stats | RX={}, CRC Err={}, LenErr={}",
        stats.pkt_rx(),
        stats.crc_error(),
        stats.len_error(),
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

async fn show_rx_pkt(lr2021: &mut Lr2021Stm32, data: &mut [u8], intr: Intr, uart: &mut Uart<'_, embassy_stm32::mode::Async>) {
    let status = lr2021.get_flrc_packet_status().await.expect("RX status");
    let nb_byte = status.pkt_len().min(PLD_SIZE) as usize; // Make sure to not read more than the local buffer size
    lr2021.rd_rx_fifo_to(&mut data[..nb_byte]).await.expect("RX FIFO Read");

    // info!("[RX] Payload = {:02x} ({}) SW{} | intr={:08x} -> {} | RSSI=-{}dBm",
    //     data[..nb_byte],
    //     status.pkt_len(),
    //     status.sw_num(),
    //     intr.value(),
    //     intr,
    //     status.rssi_avg()>>1,
    // );

    let mut s: String<128> = String::new();
    core::write!(&mut s, "{:3} ", data[0]).ok();
    if intr.header_err() || intr.crc_error() {
        core::write!(&mut s, "KO ").ok();
    } else {
        core::write!(&mut s, "OK ").ok();
    }
    core::write!(&mut s, "-{}dBm\r\n", status.rssi_avg()>>1).ok();
    uart.write(s.as_bytes()).await.ok();
}
