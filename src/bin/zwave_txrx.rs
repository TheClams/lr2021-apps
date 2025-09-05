#![no_std]
#![no_main]

//! # ZWave TX/RX demo application
//!
//! Double press change the board mode: spy or active
//! Long press while active generate a NOP command
//! While in ACTIVE mode, the board will try to answer to any Transfer Presentation command, and other related command
//! The Bare minimum support is done to appears as a binary switch and turn led on/off when requested
//! In SPY mode, the message received are decoded and print on the debug link

use defmt::*;
use embassy_time::{Duration, Timer};
use {defmt_rtt as _, panic_probe as _};

use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};

use embassy_stm32::gpio::Output;

use lr2021_apps::{board::{BoardNucleoL476Rg, ButtonPressKind, LedMode, Lr2021Stm32}, zwave_utils::{BinaryCmd, NamingCmd}};
use lr2021_apps::zwave_utils::{ProtCmd, ZwaveHdrType, ZwavePhyHdr, ManufacturerCmd, VersionCmd, ZwaveCmd};
use lr2021::radio::{FallbackMode, PaLfMode, PacketType, RampTime, RxBoost, RxPath, TimestampIndex, TimestampSource};
use lr2021::status::{Intr, IRQ_MASK_RX_DONE, IRQ_MASK_TX_DONE};
use lr2021::system::{ChipMode, DioNum};
use lr2021::zwave::*;

const NPU_NODE_INFO : [u8;12] = [
    0x01, 0x01, // OpCode NodeInfo command
    0b10011011, // Version = 3, Support all speed, no routing, listening
    0b00000000, // Unsecure end-node with no special functionality
    0x01, // Support 100kb/s
    0x00, // Static Device Type
    0x10, // Generic Device Class: Switch Binary (led on/off)
    0x00, // No Specific Device type
    0x25, // Switch Binary Command class
    0x72, // Manufacturer
    0x77, // Node Naming
    0x86, // Version
];

const PHY_HDR: ZwavePhyHdr = ZwavePhyHdr {
    home_id: 0x0184E19D,
    src: 0, //
    dst: 0xFF, // broadcast
    seq_num: 1,
    ack_req: false,
    hdr_type: ZwaveHdrType::SingleCast,
};

#[derive(Clone, Copy, Format, PartialEq)]
enum Action {
    None,
    CmdDone(u8),
    NodeInfo,
    RangeReport,
    BinaryReport,
    NameReport,
    LocReport,
    VersionCls(u8),
    Manufacturer,
    Version,
}

struct BoardState {
    is_active: bool,
    led_on: bool,
    phy_hdr: ZwavePhyHdr,
    on_tx_done: bool,
    next_action: Action,
    mode: ZwaveMode,
    trigger_tx: Output<'static>,
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Starting zwave_txrx");
    let board = BoardNucleoL476Rg::init(&spawner).await;
    let mut lr2021 = board.lr2021;
    let mut irq = board.irq;

    // Initialize transceiver for LoRa communication
    lr2021.set_rf(868_400_000).await.expect("Setting RF to 868.4MHz");
    lr2021.set_rx_path(RxPath::LfPath, RxBoost::Off).await.expect("Setting RX path to LF");
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
    lr2021.set_dio_irq(DioNum::Dio7, Intr::new(IRQ_MASK_RX_DONE|IRQ_MASK_TX_DONE)).await.expect("Setting DIO7 as IRQ");
    // Configuring timestamping of RX Packet
    lr2021.set_timestamp_source(TimestampIndex::Ts0, TimestampSource::RxDone ).await.expect("SetTs");

    let mut state = BoardState {
        is_active: false,
        led_on: false,
        phy_hdr: PHY_HDR,
        on_tx_done: false,
        next_action: Action::None,
        mode: ZwaveMode::R1,
        trigger_tx: board.trigger_tx
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
                            info!("Board in ACTIVE mode");
                        } else {
                            info!("Board in SPY mode");
                        }
                    }
                    // Long press:
                    //  - When active send a NOP to the controller
                    //  - When spy, maybe enable filtering and switch among all networked seen ?
                    ButtonPressKind::Long => {
                        if state.is_active && state.on_tx_done == false {
                            info!("Sending NodeInfo");
                            state.phy_hdr.dst = 0xFF;
                            send_message(&mut lr2021, &mut state, &NPU_NODE_INFO).await;
                        }
                    }
                }
            }
            // RX Interrupt
            Either::Second(_) => {
                let intr = lr2021.get_and_clear_irq().await.expect("Getting intr");
                // info!("Interrupt status: {} | Action={} {}", intr, state.next_action, state.on_tx_done);
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
                // If an action is pending not supposed to be trigger by TX done, execute it immediately
                if state.next_action != Action::None && ((intr.tx_done() && state.on_tx_done) || !state.on_tx_done) {
                    state.phy_hdr.hdr_type = ZwaveHdrType::SingleCast;
                    match state.next_action {
                        Action::NodeInfo => {
                            send_message(&mut lr2021, &mut state, &NPU_NODE_INFO).await;
                        }
                        Action::CmdDone(sn) => {
                            send_message(&mut lr2021, &mut state, &[1,7, sn]).await;
                        }
                        // Send a Binary report with the led status
                        Action::BinaryReport => {
                            let value = if state.led_on {0xFF} else {0x00};
                            send_message(&mut lr2021, &mut state, &[0x25,3, value]).await;
                        }
                        // Send the node name in ASCII
                        Action::NameReport => {
                            send_message(&mut lr2021, &mut state, &[0x77, 3, 0, b'R', b'i', b'g', b'i']).await;
                        }
                        // Send a Binary report with the led status
                        Action::LocReport => {
                            send_message(&mut lr2021, &mut state, &[0x77, 6, 0, b'H', b'o', b'm', b'e']).await;
                        }
                        // Dummy manufacturer report
                        Action::Manufacturer => {
                            send_message(&mut lr2021, &mut state, &[0x72,0x05, 0x00, 0x39, 0x99, 0xBA, 0xCD, 0x05]).await;
                        }
                        // Dummy version report: library 1, Version 2.36, App 1.0
                        Action::Version => {
                            send_message(&mut lr2021, &mut state, &[0x86,0x12, 1, 2, 36, 1, 0]).await;
                        }
                        // Dummy command class version: report 1 for all command class
                        Action::VersionCls(cls) => {
                            send_message(&mut lr2021, &mut state, &[0x86,0x14, cls, 1]).await;
                        }
                        // Dummy report: nothing found (we did not even tried :P)
                        Action::RangeReport    => {
                            state.phy_hdr.ack_req = true;
                            send_message(&mut lr2021, &mut state, &[1, 6, 1, 0, 0]).await;
                            state.phy_hdr.ack_req = false;
                            // After Range command node is part of the networkd -> enable address filtering
                            let scan_cfg = ZwaveScanCfg::from_region(ZwaveAddrComp::Homeid, FcsMode::Auto, ZwaveRfRegion::Eu);
                            lr2021.set_zwave_home_id(state.phy_hdr.home_id).await.expect("SetAddr");
                            lr2021.set_zwave_scan_config(&scan_cfg).await.expect("SetScan");
                        }
                        _ => lr2021.start_zwave_scan().await.expect("Scan"),
                    }
                    state.next_action = Action::None;
                } else if intr.tx_done() {
                    // info!("Scan restarted");
                    lr2021.start_zwave_scan().await.expect("Scan")
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

async fn send_message(lr2021: &mut Lr2021Stm32, state: &mut BoardState, msg: &[u8]) {
    let len = msg.len() + 9;
    lr2021.set_chip_mode(ChipMode::Fs).await.expect("SetFs");
    let params = ZwavePacketParams::from_mode(state.mode, ZwavePpduKind::SingleCast, len as u8);
    lr2021.set_zwave_packet(&params).await.expect("SetPacket");
    lr2021.buffer_mut()[..9].copy_from_slice(&state.phy_hdr.to_bytes((len+1) as u8)); // +1 for CRC
    if len > 9 {
        lr2021.buffer_mut()[9..len].copy_from_slice(msg);
    }
    lr2021.wr_tx_fifo(len).await.expect("FIFO write");
    // For Ack packet we need to respect some precise timing: check timestamp and use a TX trigger
    if state.phy_hdr.hdr_type == ZwaveHdrType::Ack {
        let rx_ts_tick = lr2021.get_timestamp(TimestampIndex::Ts0).await.expect("GetTs");
        let rx_ts_ns = (rx_ts_tick as u64 * 125) >> 2; // 32MHz -> 31.25ns
        // Ensure the packet will starts after ~ 1ms
        let sleep = Duration::from_micros(1000) - Duration::from_nanos(rx_ts_ns);
        Timer::after(sleep).await;
        state.trigger_tx.set_high();
        lr2021.wait_ready(Duration::from_micros(100)).await.expect("WaitTxTrigger");
        Timer::after_micros(1).await;
        state.trigger_tx.set_low();
    } else {
        lr2021.set_tx(0).await.expect("SetTx");
    }
}


async fn handle_rx_pkt(lr2021: &mut Lr2021Stm32, state: &mut BoardState) {
    let status = lr2021.get_zwave_packet_status().await.expect("RX status");
    let nb_byte = status.pkt_len() as usize; // Make sure to not read more than the local buffer size
    lr2021.rd_rx_fifo(nb_byte).await.expect("RX FIFO Read");

    let lqi = status.lqi();
    let lqi_frac = (lqi&3) * 25;

    if let Some(rx_phy_hdr) = ZwavePhyHdr::parse(lr2021.buffer()) {
        let npdu = &lr2021.buffer()[9..nb_byte.max(9)];
        let cmd = ZwaveCmd::parse(npdu);
        // Extremly basic handling of some ZWave command to join a network
        if state.is_active {
            // Send packet to the source with an unitialized source address
            let ctrl_id = rx_phy_hdr.src;
            // Default destination to TX node
            state.phy_hdr.dst = ctrl_id;
            state.phy_hdr.seq_num = rx_phy_hdr.seq_num;
            // Clear state action
            state.on_tx_done = false;
            state.next_action = Action::None;
            // Use same rate as sender
            state.mode = status.last_detect();
            // On Set ID update local info
            if cmd == ZwaveCmd::Prot(ProtCmd::SetId) {
                state.phy_hdr.src = *npdu.get(2).unwrap_or(&0);
                state.phy_hdr.home_id = rx_phy_hdr.home_id; // In theory should take home ID referenced in NPDU
                info!(" - Joining HomeID {} with ID {}", state.phy_hdr.home_id, state.phy_hdr.src);
            }
            // Send Ack when requested
            if rx_phy_hdr.ack_req {
                state.phy_hdr.hdr_type = ZwaveHdrType::Ack;
                send_message(lr2021, state, &[]).await;
            } else {
                state.phy_hdr.hdr_type = ZwaveHdrType::SingleCast;
            }
            // Handle the command
            match cmd {
                ZwaveCmd::Prot(ProtCmd::TransferPres) |
                ZwaveCmd::Prot(ProtCmd::ReqInfo) => {
                    state.phy_hdr.home_id = rx_phy_hdr.home_id;
                    state.next_action = Action::NodeInfo;
                }
                // On FindNode simply answer job done
                // either immediately or postponed after ack
                ZwaveCmd::Prot(ProtCmd::FindNodesInRange) => {
                    let sn = rx_phy_hdr.seq_num;
                    state.next_action = Action::CmdDone(sn);
                }
                // On GetNode answer no node were found
                ZwaveCmd::Prot(ProtCmd::GetNodesInRange) => {
                    state.next_action = Action::RangeReport;
                }
                // On GetNode answer no node were found
                ZwaveCmd::Manufacturer(ManufacturerCmd::Get) => {
                    state.next_action = Action::Manufacturer;
                }
                // On GetNode answer no node were found
                ZwaveCmd::Version(VersionCmd::Get) => {
                    state.next_action = Action::Version;
                }
                // On GetNode answer no node were found
                ZwaveCmd::Version(VersionCmd::ClassGet(cls)) => {
                    state.next_action = Action::VersionCls(cls);
                }
                // On GetNode answer no node were found
                ZwaveCmd::Binary(binary_cmd) => {
                    match binary_cmd {
                        BinaryCmd::SetOff => {
                            BoardNucleoL476Rg::led_red_set(LedMode::Off);
                            state.led_on = false;
                        }
                        BinaryCmd::SetOn  => {
                            BoardNucleoL476Rg::led_red_set(LedMode::On);
                            state.led_on = true;
                        }
                        BinaryCmd::Get => {
                            state.next_action = Action::BinaryReport;
                        }
                        _ => {}
                    }
                }
                // Support the Name/Loc get
                ZwaveCmd::Naming(naming_cmd) => {
                    match naming_cmd {
                        NamingCmd::NameGet => state.next_action = Action::NameReport,
                        NamingCmd::LocGet  => state.next_action = Action::LocReport,
                        _ => {}
                    }
                }
                // Ignore unknown commands
                _ => {}
            }
            // Delay the action on TX done if an ACK was requested
            state.on_tx_done = state.next_action!=Action::None && rx_phy_hdr.ack_req;
        }
        if rx_phy_hdr.hdr_type == ZwaveHdrType::Ack {
            info!("{} | {}", status.last_detect(), rx_phy_hdr);
        } else {
            info!("{} | {} : {} | Next: {} ({}) | {:02x}",
                status.last_detect(),
                rx_phy_hdr, cmd,
                state.next_action, state.on_tx_done,
                &lr2021.buffer()[9..nb_byte.max(9)] // Note: Still valid even if the ACK was sent
            );
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
