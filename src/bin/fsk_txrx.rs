#![no_std]
#![no_main]

//! # FSK TX/RX demo application
//!
//! Slow blinking led green is for RX, red is for TX
//! In RX mode, the red led flash when a CRC error is detected, and the green led flash on CRC OK
//! Long press on user button switch the board role between TX and RX
//! Short press either send a packet of incrementing byte or display RX stats in RX
//!
//! The board also accept command by UART (running at 444_444bauds), one character per command:
//!  - 's' to switch mode
//!  - 't' to transmit a packet
//!  - 'a' to toggle auto mode in transmit to have one packet every 250ms
//!  - 'h' to alternate between two modulation index (0.5 and 1.0)

use defmt::*;
use embassy_stm32::{mode::Async, usart::Uart};
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

use embassy_executor::Spawner;
use embassy_futures::select::{select3, Either3};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};

use lr2021_apps::board::{BoardNucleoL476Rg, BoardRole, ButtonPressKind, LedMode, Lr2021Stm32};
use lr2021::{
    fsk::{AddrComp, BitOrder, Crc, FskPktFormat, PblLenDetect, PldLenUnit},
    radio::{PacketType, RampTime, RxBoost, RxPath},
    status::{Intr, IRQ_MASK_RX_DONE, IRQ_MASK_TX_DONE},
    system::{ChipMode, DioNum}, PulseShape, RxBw
};

const PLD_SIZE : u8 = 10;

#[derive(Debug, Clone, Copy, Format)]
enum UartCmd {
    SwitchTxRx, ChangeModIdx, ToggleAuto, StartTx, Invalid
}
type SignalCmd = Signal<CriticalSectionRawMutex, UartCmd>;
static CMD : SignalCmd = Signal::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Starting fsk_txrx");

    let board = BoardNucleoL476Rg::init(&spawner).await;
    let mut lr2021 = board.lr2021;
    let mut irq = board.irq;

    spawner.spawn(handle_uart(board.uart, &CMD)).unwrap();

    // Packet ID: correspond to first byte sent
    let mut pkt_id = 0_u8;
    let mut fdev = 62500;

    // Initialize transceiver for FSK communication
    // 901MHz, 0dbM, SF5 BW1000, CR 4/5
    lr2021.set_rf(901_000_000).await.expect("Setting RF to 901MHz");
    lr2021.set_rx_path(RxPath::LfPath, RxBoost::Off).await.expect("Setting RX path to LF");
    lr2021.calib_fe(&[]).await.expect("Front-End calibration");

    match lr2021.get_status().await {
        Ok((status, intr)) => info!("Calibration Done: {} | {}", status, intr),
        Err(e) => warn!("Calibration Failed: {}", e),
    }

    lr2021.set_packet_type(PacketType::FskLegacy).await.expect("SetPktType");
    lr2021.set_fsk_modulation(250_000, PulseShape::Bt0p5, RxBw::Bw444, fdev).await.expect("SetFskModulation");
    lr2021.set_fsk_syncword(0xCD05DEAD, BitOrder::LsbFirst, 32).await.expect("SetSyncword");
    lr2021.set_fsk_packet(8, PblLenDetect::None, false, PldLenUnit::Bytes, AddrComp::Off, FskPktFormat::Variable8bit, 10, Crc::Crc2Byte, true).await.expect("SetPkt");
    lr2021.set_tx_params(0, RampTime::Ramp8u).await.expect("Setting TX parameters");

    // Start RX continuous
    match lr2021.set_rx(0xFFFFFFFF, true).await {
        Ok(_) => info!("[RX] Searching Preamble"),
        Err(e) => error!("Fail while set_rx() : {}", e),
    }

    // Set DIO7 as IRQ for RX Done
    lr2021.set_dio_irq(DioNum::Dio7, Intr::new(IRQ_MASK_RX_DONE|IRQ_MASK_TX_DONE)).await.expect("Setting DIO7 as IRQ");

    // Wait for a button press for actions
    let mut button_press = BoardNucleoL476Rg::get_button_evt();
    BoardNucleoL476Rg::led_green_set(LedMode::BlinkSlow);

    let mut role = BoardRole::Rx;
    let mut auto_tx = true;

    loop {
        match select3(button_press.changed(), irq.wait_for_rising_edge(), CMD.wait()).await {
            Either3::First(press) => {
                match (press, role) {
                    // Short press in RX => clear stats
                    (ButtonPressKind::Short, BoardRole::Rx) => show_and_clear_rx_stats(&mut lr2021).await,
                    // Short press in TX => send a packet
                    (ButtonPressKind::Short, BoardRole::Tx) => {
                        send_pkt(&mut lr2021, &mut pkt_id).await;
                    }
                    (ButtonPressKind::Double, BoardRole::Tx) => {
                        auto_tx = !auto_tx;
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
            Either3::Second(_) => {
                let intr = lr2021.get_and_clear_irq().await.expect("GetIrqs");
                if intr.tx_done() {
                    BoardNucleoL476Rg::led_red_set(LedMode::Flash);
                    if auto_tx {
                        Timer::after_millis(250).await;
                        send_pkt(&mut lr2021, &mut pkt_id).await;
                    }

                } else if !intr.crc_error() {
                    BoardNucleoL476Rg::led_green_set(LedMode::Flash);
                    show_rx_pkt(&mut lr2021).await;
                } else {
                    warn!("CRC Error");
                    lr2021.clear_rx_fifo().await.unwrap();
                }
            }
            // UART command
            Either3::Third(cmd) => {
                match cmd {
                    UartCmd::SwitchTxRx => {
                        role.toggle();
                        switch_mode(&mut lr2021, role.is_rx()).await;
                    }
                    UartCmd::ChangeModIdx => {
                        lr2021.set_chip_mode(ChipMode::Fs).await.expect("SetFs");
                        fdev = if fdev == 125000 {62500} else {125000};
                        info!("Changing FDev tp {}kHz", fdev);
                        lr2021.set_fsk_modulation(250_000, PulseShape::Bt0p5, RxBw::Bw444, fdev).await.expect("SetFskModulation");
                        if role.is_rx() {
                            lr2021.set_rx(0xFFFFFFFF, true).await.expect("SetRx");
                        }
                    }
                    UartCmd::ToggleAuto => {
                        auto_tx = !auto_tx;
                        info!("Auto Mode {}", auto_tx);
                    }
                    UartCmd::StartTx => send_pkt(&mut lr2021, &mut pkt_id).await,
                    UartCmd::Invalid => {},
                }
            }
        }
    }
}

async fn show_and_clear_rx_stats(lr2021: &mut Lr2021Stm32) {
    let stats = lr2021.get_fsk_rx_stats().await.expect("RX stats");
    info!("[RX] Clearing stats | RX={}, CRC Err={}, LenErr={} | Detect={}, SyncFail={}",
        stats.pkt_rx(),
        stats.crc_error(),
        stats.len_error(),
        stats.pbl_det(),
        stats.sync_fail(),
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
        BoardNucleoL476Rg::led_red_set(LedMode::BlinkSlow);
        BoardNucleoL476Rg::led_green_set(LedMode::Off);
        info!(" -> Ready for TX: press button to start periodic packet transmission");
    }
}

async fn show_rx_pkt(lr2021: &mut Lr2021Stm32) {
    let pkt_len = lr2021.get_rx_pkt_len().await.expect("RX Fifo level") as usize;
    let status = lr2021.get_fsk_packet_status().await.expect("RX status");
    lr2021.rd_rx_fifo(pkt_len).await.expect("RX FIFO Read");
    let lqi = status.lqi();
    let lqi_frac = (lqi&3) * 25;
    info!("[RX] Payload = {:02x} | RSSI=-{}dBm, LQI={}.{:02}",
        lr2021.buffer()[..pkt_len],
        status.rssi_avg()>>1,
        lqi>>2, lqi_frac
    );
}

#[embassy_executor::task]
pub async fn handle_uart(mut uart: Uart<'static, Async>, sig_cmd: &'static SignalCmd) {
    loop {
        // Wait for a command
        let mut buffer = [0u8;8];
        uart.read_until_idle(&mut buffer).await.ok();
        // Parsing: either R[min]-[max] or S[step]
        let cmd = match buffer[0] {
            b'S' | b's' => UartCmd::SwitchTxRx,
            b'T' | b't' => UartCmd::StartTx,
            b'A' | b'a' => UartCmd::ToggleAuto,
            b'H' | b'h' => UartCmd::ChangeModIdx,
            _ => UartCmd::Invalid,
        };
        // info!("[UART] Command = {}", cmd);
        uart.write(&buffer[0..1]).await.ok();
        sig_cmd.signal(cmd);
    }
}
