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

use lr2021_apps::board::{BoardNucleoL476Rg, ButtonPressKind, LedMode, Lr2021Stm32};
// use lr2021_apps::board::*;
use lr2021_apps::zwave_utils::{ProtCmd, ZwaveHdrType, ZwavePhyHdr, ManufacturerCmd, VersionCmd, ZwaveCmd};
use lr2021::radio::{FallbackMode, PaLfMode, PacketType, RampTime, RxPath};
use lr2021::status::{Intr, IRQ_MASK_RX_DONE, IRQ_MASK_TX_DONE};
use lr2021::system::ChipMode;
use lr2021::zwave::*;

const NPU_NODE_INFO : [u8;11] = [
    0x01, 0x01, // OpCode NodeInfo command
    0b10011011, // Version = 3, Support all speed, no routing, listening
    0b00000000, // Unsecure end-node with no special functionality
    0x01, // Support 100kb/s
    0x00, // Static Device Type
    0x10, // Generic Device Class: Switch Binary (led on/off)
    0x00, // No Specific Device type
    0x01, // Support Set command
    0x02, // Support Get Command
    0x03, // Support Report command
];

const PHY_HDR: ZwavePhyHdr = ZwavePhyHdr {
    home_id: 0x0184E19D,
    src: 0, // uninit
    dst: 0xFF, // broadcast
    seq_num: 1,
    ack_req: false,
    hdr_type: ZwaveHdrType::SingleCast,
};

#[derive(Clone, Copy, Format, PartialEq)]
enum Action {
    None,
    CmdDone(u8),
    RangeRes,
    Manufacturer,
    Version,
}

struct BoardState {
    is_active: bool,
    phy_hdr: ZwavePhyHdr,
    on_tx_done: Action,
    mode: ZwaveMode,
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Starting zwave_txrx");
    let board = BoardNucleoL476Rg::init(&spawner).await;
    let mut lr2021 = board.lr2021;
    let mut irq = board.irq;

    // Initialize transceiver for LoRa communication
    lr2021.set_rf(868_400_000).await.expect("Setting RF to 868.4MHz");
    lr2021.set_rx_path(RxPath::LfPath, 0).await.expect("Setting RX path to LF");
    // lr2021.set_rf(2_400_000_000).await.expect("Setting RF to 2.4GHz");
    // lr2021.set_rx_path(RxPath::HfPath, 0).await.expect("Setting RX path to HF");
    lr2021.calib_fe(&[]).await.expect("Front-End calibration");
    match lr2021.get_status().await {
        Ok((status, intr)) => info!("Calibration Done: {} | {}", status, intr),
        Err(e) => warn!("Calibration Failed: {}", e),
    }

    lr2021.set_pa_lf(PaLfMode::LfPaFsm, 6, 7).await.expect("Set PA HF");
    lr2021.set_tx_params(0, RampTime::Ramp8u).await.expect("SetTxParam");
    lr2021.set_fallback(FallbackMode::Fs).await.expect("Set fallback");

    // Configure ZWave: scan in EU
    lr2021.set_packet_type(PacketType::Zwave).await.expect("SetPkt");
    let scan_cfg = ZwaveScanCfg::from_region(ZwaveAddrComp::Off, FcsMode::Auto, ZwaveRfRegion::Eu);
    lr2021.set_zwave_scan_config(&scan_cfg).await.expect("SetScan");
    lr2021.start_zwave_scan().await.expect("Scan");

    // Check for error after configuration
    let intr = lr2021.get_and_clear_irq().await.expect("Getting intr");
    if intr.error() {
        let rsp = lr2021.get_errors().await.expect("GetErrors");
        warn!("Error = {:08x} => {}", rsp.value(), rsp);
    }

    // Set DIO7 as IRQ for RX Done
    lr2021.set_dio_irq(7, Intr::new(IRQ_MASK_RX_DONE|IRQ_MASK_TX_DONE)).await.expect("Setting DIO7 as IRQ");

    let mut state = BoardState{
        is_active: false,
        phy_hdr: PHY_HDR,
        on_tx_done: Action::None,
        mode: ZwaveMode::R1
    };

    BoardNucleoL476Rg::led_green_set(LedMode::BlinkSlow);
    // Wait for a button press for actions
    let mut button_press = BoardNucleoL476Rg::get_button_evt();

    loop {
        match select(button_press.changed(), irq.wait_for_rising_edge()).await {
            Either::First(press) => {
                match press {
                    // Short press => show stats
                    ButtonPressKind::Short => show_and_clear_rx_stats(&mut lr2021).await,
                    // Double press => Change between spy and active node
                    ButtonPressKind::Double => {
                        state.is_active = !state.is_active;
                        if state.is_active {
                            info!("Board in JOIN mode");
                        } else {
                            info!("Board in SPY mode");
                        }
                    }
                    // Long press:
                    //  - When active send a NOP to the controller
                    //  - When spy, maybe enable filtering and switch among all networked seen ?
                    ButtonPressKind::Long => {
                        if state.is_active && state.on_tx_done == Action::None {
                            info!("Sending NodeInfo");
                            state.phy_hdr.dst = 0xFF;
                            send_message(&mut lr2021, &state, &NPU_NODE_INFO).await;
                        }
                    }
                }
            }
            // RX Interrupt
            Either::Second(_) => {
                let intr = lr2021.get_and_clear_irq().await.expect("Getting intr");
                // info!("Interrupt status: {}", intr);
                if intr.error() {
                    let rsp = lr2021.get_errors().await.expect("GetErrors");
                    warn!("Error = {:08x} => {}", rsp.value(), rsp);
                }
                if intr.rx_done() && ! intr.addr_error() {
                    handle_rx_pkt(&mut lr2021, &mut state).await;
                    if intr.crc_error() {
                        BoardNucleoL476Rg::led_red_set(LedMode::Flash);
                    } else {
                        BoardNucleoL476Rg::led_green_set(LedMode::Flash);
                    }
                }
                lr2021.clear_rx_fifo().await.expect("ClearFifo");
                // On TxDone either go in scan or send another command (happens after an ack typically)
                if intr.tx_done() {
                    // info!("TX Done: {}", state.on_tx_done);
                    state.phy_hdr.hdr_type = ZwaveHdrType::SingleCast;
                    match state.on_tx_done {
                        Action::CmdDone(sn) => {
                            send_message(&mut lr2021, &state, &[1,7, sn]).await;
                        }
                        // Dummy manufacturer report
                        Action::Manufacturer => {
                            send_message(&mut lr2021, &state, &[0x72,0x05, 0x00, 0x39, 0x99, 0xBA, 0xCD, 0x05]).await;
                        }
                        // Dummy version report: library 1, Version 2.36, App 1.0
                        Action::Version => {
                            send_message(&mut lr2021, &state, &[0x86,0x12, 1, 2, 36, 1, 0]).await;
                        }
                        Action::RangeRes    => {
                            state.phy_hdr.ack_req = true;
                            send_message(&mut lr2021, &state, &[1, 6, 1, 0, 0]).await;
                            state.phy_hdr.ack_req = false;
                            // After Range command node is part of the networkd -> enable address filtering
                            let scan_cfg = ZwaveScanCfg::from_region(ZwaveAddrComp::Homeid, FcsMode::Auto, ZwaveRfRegion::Eu);
                            lr2021.set_zwave_home_id(state.phy_hdr.home_id).await.expect("SetAddr");
                            lr2021.set_zwave_scan_config(&scan_cfg).await.expect("SetScan");
                        }
                        _ => lr2021.start_zwave_scan().await.expect("Scan"),
                    }
                    state.on_tx_done = Action::None;
                }
            }
        }
    }
}

async fn show_and_clear_rx_stats(lr2021: &mut Lr2021Stm32) {
    let stats = lr2021.get_zwave_rx_stats_adv().await.expect("RX stats");
    info!("[RX] Clearing stats | RX={}, CRC Err={}, LenErr={}, SyncFail={}, Timeout={}",
        stats.pkt_rx(),
        stats.crc_error(),
        stats.len_error(),
        stats.sync_fail(),
        stats.timeout(),
    );
    lr2021.clear_rx_stats().await.unwrap();
}

async fn send_message(lr2021: &mut Lr2021Stm32, state: &BoardState, msg: &[u8]) {
    let len = msg.len() + 9;
    lr2021.set_chip_mode(ChipMode::Fs).await.expect("SetFs");
    let params = ZwavePacketParams::from_mode(state.mode, ZwavePpduKind::SingleCast, len as u8);
    lr2021.set_zwave_packet(&params).await.expect("SetPacket");
    lr2021.buffer_mut()[..9].copy_from_slice(&state.phy_hdr.to_bytes((len+1) as u8)); // +1 for CRC
    if len > 9 {
        lr2021.buffer_mut()[9..len].copy_from_slice(msg);
    }
    lr2021.wr_tx_fifo(len).await.expect("FIFO write");
    lr2021.set_tx(0).await.expect("SetTx");
}


async fn handle_rx_pkt(lr2021: &mut Lr2021Stm32, state: &mut BoardState) {
    let t0 = embassy_time::Instant::now();
    let status = lr2021.get_zwave_packet_status().await.expect("RX status");
    let t1 = embassy_time::Instant::now();
    let nb_byte = status.pkt_len() as usize; // Make sure to not read more than the local buffer size
    lr2021.rd_rx_fifo(nb_byte).await.expect("RX FIFO Read");
    let t2 = embassy_time::Instant::now();

    let lqi = status.lqi();
    let lqi_frac = (lqi&3) * 25;

    if let Some(rx_phy_hdr) = ZwavePhyHdr::parse(lr2021.buffer()) {
        let npu = &lr2021.buffer()[9..nb_byte.max(9)];
        let cmd = ZwaveCmd::parse(npu);
        let handled = state.is_active && (matches!(cmd, ZwaveCmd::Prot(ProtCmd::TransferPres)) || rx_phy_hdr.ack_req);
        // Extremly basic handling of some ZWave command to join a network
        if state.is_active {
            // Save command arguments (5 bytes for the moment)
            let args : [u8;5] = core::array::from_fn(|i| npu.get(2+i).copied().unwrap_or(0));
            // Send packet to the source with an unitialized source address
            let ctrl_id = rx_phy_hdr.src;
            // Default destination to TX node
            state.phy_hdr.dst = ctrl_id;
            state.phy_hdr.seq_num = rx_phy_hdr.seq_num;
            state.on_tx_done = Action::None;
            state.mode = status.last_detect();
            if cmd == ZwaveCmd::Prot(ProtCmd::SetId) {
                state.phy_hdr.src = *npu.get(2).unwrap_or(&0);
                info!(" - Setting ID to {} -> {}", args[0], state.phy_hdr);
            }
            // Send Ack when requested
            if rx_phy_hdr.ack_req {
                state.phy_hdr.hdr_type = ZwaveHdrType::Ack;
                let t3 = embassy_time::Instant::now();
                send_message(lr2021, &state, &[]).await;
                let dt  = (embassy_time::Instant::now() - t0).as_micros();
                let dt1 = (t1 - t0).as_micros();
                let dt2 = (t2 - t1).as_micros();
                let dt3 = (t3 - t2).as_micros();
                info!("Ack sent after {}us | breakdown: {}, {}, {}", dt, dt1, dt2, dt3);
            } else {
                state.phy_hdr.hdr_type = ZwaveHdrType::SingleCast;
            }
            // Handle the command
            match cmd {
                ZwaveCmd::Prot(ProtCmd::TransferPres) => {
                    state.phy_hdr.home_id = rx_phy_hdr.home_id;
                    send_message(lr2021, &state, &NPU_NODE_INFO).await;
                }
                // On FindNode simply answer job done
                // either immediately or postponed after ack
                ZwaveCmd::Prot(ProtCmd::FindNodesInRange) => {
                    let sn = rx_phy_hdr.seq_num;
                    if rx_phy_hdr.ack_req {
                        state.on_tx_done = Action::CmdDone(sn);
                    } else {
                        send_message(lr2021, &state, &[1,7, sn]).await;
                    }
                }
                // On GetNode answer no node were found
                ZwaveCmd::Prot(ProtCmd::GetNodesInRange) => {
                    if rx_phy_hdr.ack_req {
                        state.on_tx_done = Action::RangeRes;
                    } else {
                        send_message(lr2021, &state, &[1,6,0]).await;
                    }
                }
                // On GetNode answer no node were found
                ZwaveCmd::Manufacturer(ManufacturerCmd::Get) => {
                    if rx_phy_hdr.ack_req {
                        state.on_tx_done = Action::Manufacturer;
                    } else {
                         send_message(lr2021, &state, &[0x72,0x05, 0x00, 0x39, 0x99, 0xBA, 0xCD, 0x05]).await;
                    }
                }
                // On GetNode answer no node were found
                ZwaveCmd::Version(VersionCmd::Get) => {
                    if rx_phy_hdr.ack_req {
                        state.on_tx_done = Action::Version;
                    } else {
                          send_message(lr2021, &state, &[0x86,0x12, 1, 2, 36, 1, 0]).await;
                    }
                }
                _ => {}
            }
            if rx_phy_hdr.hdr_type == ZwaveHdrType::Ack {
                info!("{} | {} ", state.mode, rx_phy_hdr);
            } else {
                info!("{} | {} : {} {}",
                    status.last_detect(),
                    rx_phy_hdr, cmd,
                    if handled {"| Responded"} else {""}
                );
            }
        }
        // Display info
        else if rx_phy_hdr.hdr_type == ZwaveHdrType::Ack {
            info!("{} | {}", status.last_detect(), rx_phy_hdr);
        } else {
            info!("{} | {} : {} : {:02x}",
                status.last_detect(),
                rx_phy_hdr, cmd, npu);
        }
    } else {
        info!("[Raw] {} {:02x}",
            status.last_detect(),
            lr2021.buffer()[..nb_byte],
        );
    }
    // Show RSSI / LQI of last packet received
    info!("     - RSSI=-{}dBm, LQI={}.{}", status.rssi_avg()>>1, lqi>>1, lqi_frac);
}
