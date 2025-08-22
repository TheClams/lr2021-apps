#![no_std]
#![no_main]

// BLE TX/RX Demo application
// Long press on user button switch the board role between TX and RX
// Double press change the advertising channel (OOB/37/38/39)
// Short press while in TX mode, send an packet advertising packet
// Short press while in RX mode, switch to scan mode on all recently seens address

use defmt::*;
use {defmt_rtt as _, panic_probe as _};

use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_stm32::{mode::Async};
use embassy_stm32::spi::{Config, Spi};
use embassy_stm32::{
    exti::ExtiInput,
    gpio::{Level, Output, Pull, Speed},
    time::Hertz,
};
use embassy_sync::{signal::Signal, watch::Watch};

use lr2021_apps::{
    ble_adv::{parse_and_print_ble_adv, parse_ble_adv_hdr, print_ble_adv, AddrList, BleAdvType},
    board::{blink, user_intf, BoardRole, ButtonPressKind, LedMode, SignalLedMode, WatchButtonPress},
};
use lr2021::{
    ble::*,
    radio::{FallbackMode, PacketType, RampTime, RxPath},
    status::{Intr, IRQ_MASK_RX_DONE, IRQ_MASK_TX_DONE},
    system::ChipMode, BusyAsync, Lr2021
};

const VERBOSE: bool = false;

/// Packet sent in TX mode
const ADV_BEACON : [u8;28] = [
    // Header: 2=ADV_IND, with 26 bytes
    0x00, 26,
    // Advertising Address on 6B
    0xa4, 0x63, 0xef, 0x8c, 0x89, 0xe6,
    // Advertising flags
    0x02, 0x01, 0x06,
    // List of 16b Service class UUIDs (Human Interface Device)
    0x03, 0x03, 0x12, 0x18,
    // Short name = Clams
    0x06, 0x08, b'C', b'l', b'a', b'm', b's',
    // 0x06, 0x08, 0x43, 0x6C, 0x61, 0x6D, 0x73,
    // Manufacturer = ST Microelectronics
    0x05, 0xFF, 0x30, 0x00, 0xCD, 0x05
];

/// Generate event when the button is press with short (0) or long (1) duration
static BUTTON_PRESS: WatchButtonPress = Watch::new();
/// Led modes
static LED_TX_MODE: SignalLedMode = Signal::new();
static LED_RX_MODE: SignalLedMode = Signal::new();

#[derive(Debug, Clone, Copy, PartialEq, Format)]
pub enum AdvChanRf {Chan37, Chan38, Chan39, ChanOob}

impl AdvChanRf {
    pub fn freq(&self) -> u32 {
        match self {
            AdvChanRf::Chan37 => 2_402_000_000,
            AdvChanRf::Chan38 => 2_426_000_000,
            AdvChanRf::Chan39 => 2_480_000_000,
            AdvChanRf::ChanOob => 2_300_000_000,
        }
    }
    pub fn whit_init(&self) -> u8 {
        match self {
            AdvChanRf::Chan37 => 0x53,
            AdvChanRf::Chan38 => 0x33,
            AdvChanRf::Chan39 => 0x73,
            AdvChanRf::ChanOob => 0xCD,
        }
    }
    pub fn next(&mut self) {
        *self = match self {
            AdvChanRf::Chan37 => AdvChanRf::Chan38,
            AdvChanRf::Chan38 => AdvChanRf::Chan39,
            AdvChanRf::Chan39 => AdvChanRf::ChanOob,
            AdvChanRf::ChanOob => AdvChanRf::Chan37,
        }
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());
    info!("Starting ble_txrx");

    // Start tasks to blink the TX/RX leds
    let led_tx = Output::new(p.PC1, Level::High, Speed::Low);
    spawner.spawn(blink(led_tx, &LED_TX_MODE)).unwrap();
    LED_TX_MODE.signal(LedMode::Off);

    let led_rx = Output::new(p.PC0, Level::High, Speed::Low);
    spawner.spawn(blink(led_rx, &LED_RX_MODE)).unwrap();
    LED_RX_MODE.signal(LedMode::BlinkSlow);

    // Start task to check the button press
    let button = ExtiInput::new(p.PC13, p.EXTI13, Pull::Up);
    spawner.spawn(user_intf(button, &BUTTON_PRESS)).unwrap();

    // Control pins
    let busy = ExtiInput::new(p.PB3, p.EXTI3, Pull::Up);
    let nreset = Output::new(p.PA0, Level::High, Speed::Low);

    let mut irq = ExtiInput::new(p.PB0, p.EXTI0, Pull::None); // DIO7

    // SPI
    let mut spi_config = Config::default();
    spi_config.frequency = Hertz(4_000_000);
    let spi = Spi::new(
        p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA1_CH3, p.DMA1_CH2, spi_config,
    );
    let nss = Output::new(p.PA8, Level::High, Speed::VeryHigh);

    // Create driver and reset board
    let mut lr2021 = Lr2021::new(nreset, busy, spi, nss);
    lr2021.reset().await.expect("Resetting chip !");

    // Check version
    let version = lr2021.get_version().await.expect("Reading firmware version !");
    info!("FW Version {}", version);

    // Select Out-of-band channel to avoid immediately picking BLE traffic and allow board-to-board communication
    let mut chan = AdvChanRf::ChanOob;

    // Wait for a button press for actions
    let mut button_press = BUTTON_PRESS.receiver().unwrap();

    // Initialize transceiver for BLE communication with max boost
    lr2021.set_rf(chan.freq()).await.expect("SetRF");
    lr2021.set_rx_path(RxPath::HfPath, 7).await.expect("Setting RX path to HF");
    lr2021.calib_fe(&[]).await.expect("Front-End calibration");

    match lr2021.get_status().await {
        Ok((status, intr)) => info!("Calibration Done: {} | {}", status, intr),
        Err(e) => warn!("Calibration Failed: {}", e),
    }

    lr2021.set_pa_hf().await.expect("Set PA HF");
    lr2021.set_tx_params(0, RampTime::Ramp4u).await.expect("Setting TX parameters");

    // Stay in FS between packets to be more reactive
    lr2021.set_fallback(FallbackMode::Fs).await.expect("Set fallback");

    // Start RX continuous
    lr2021.set_packet_type(PacketType::Ble).await.expect("Setting packet type to BLE");
    lr2021.set_ble_modulation(BleMode::Le1mb).await.expect("Setting BLE mode (1Mb/s)");
    set_ble_chan(&mut lr2021, chan).await;

    lr2021.set_rx(0xFFFFFFFF, true).await.expect("SetRX");

    // Set DIO7 as IRQ for TX/RX Done
    lr2021.set_dio_irq(7, Intr::new(IRQ_MASK_TX_DONE|IRQ_MASK_RX_DONE)).await.expect("Setting DIO7 as IRQ");

    // Keep a list of address seen to avoid spamming
    let mut addr_seen = AddrList::new(0xa463ef8c89e6);

    let mut role = BoardRole::Rx;

    loop {
        match select(button_press.changed(), irq.wait_for_high()).await {
            Either::First(press) => {
                match (press, role) {
                    // Short press in TX => send a packet
                    (ButtonPressKind::Short, BoardRole::Tx) => send_beacon(&mut lr2021).await,
                    // Short press in RX => Show stats
                    (ButtonPressKind::Short, BoardRole::Rx|BoardRole::TxAuto) => {
                        let stat = lr2021.get_ble_rx_stats_adv().await.expect("RX Stats");
                        addr_seen.clear();
                        role.toggle_auto();
                        info!("[RX] Switching to {} | Stats: RX={}, CRC ok={}, CRC err={}, Len err={}, Sync Fail={}",
                            role, stat.pkt_rx(), stat.crc_ok(), stat.crc_error(), stat.len_error(), stat.sync_fail());
                    }
                    // Long press: switch role TX/RX
                    (ButtonPressKind::Long, _) => {
                        role.toggle();
                        switch_mode(&mut lr2021, chan, role.is_rx()).await;
                    }
                    // Double press => change channel
                    (ButtonPressKind::Double, r) => {
                        chan.next();
                        switch_channel(&mut lr2021, chan, &addr_seen, r.is_rx()).await;
                    }
                }
                // Clear address list in RX after a long or double button press
                if role.is_rx() && !press.is_short() {
                    addr_seen.clear();
                }
            }
            // Interrupt
            Either::Second(_) => {
                // Clear all IRQs
                let intr = lr2021.get_and_clear_irq().await.expect("GetIrqs");
                if intr.tx_done() {
                    LED_TX_MODE.signal(LedMode::Flash);
                }
                // Make sure the FIFO contains data
                let lvl = lr2021.get_rx_fifo_lvl().await.expect("RxFifoLvl");
                if lvl > 0 && intr.rx_done() {
                    if let Some(pkt_status) = read_pkt(&mut lr2021, intr).await {
                        let nb_byte = pkt_status.pkt_len().min(128) as usize;
                        let rssi_dbm = pkt_status.rssi_avg()>>1;
                        if role==BoardRole::TxAuto {
                            // In Tx Auto mode, parse the header
                            if let Some((hdr, addr)) = parse_ble_adv_hdr(&lr2021.buffer()[..nb_byte]) {
                                lr2021.set_chip_mode(ChipMode::Fs).await.expect("SetFs");
                                match hdr.get_type() {
                                    BleAdvType::AdvInd |
                                    BleAdvType::AdvDirectInd => send_req(&mut lr2021, BleAdvType::ConnectInd, addr).await,
                                    BleAdvType::AdvScanInd   => send_req(&mut lr2021, BleAdvType::ScanReq, addr).await,
                                    _ => {
                                        print_ble_adv(&mut addr_seen, &lr2021.buffer()[..nb_byte], hdr, addr, rssi_dbm);
                                    }
                                }
                                // Back to RX Continuous
                                lr2021.set_rx(0xFFFFFFFF, true).await.expect("SetRx");
                            }
                        } else {
                            parse_and_print_ble_adv(&mut addr_seen, &lr2021.buffer()[..nb_byte], rssi_dbm, VERBOSE);
                        }
                        // show_rx_pkt(&mut lr2021, &mut data, &mut addr_seen, intr, VERBOSE).await;
                        if !intr.crc_error() {
                            LED_RX_MODE.signal(LedMode::Flash);
                        }
                    }
                }
            }
        }
    }
}

type Lr2021Stm32 = Lr2021<Output<'static>,Spi<'static, Async>, BusyAsync<ExtiInput<'static>>>;

async fn set_ble_chan(lr2021: &mut Lr2021Stm32, chan: AdvChanRf) {
    lr2021.set_ble_params(false, ChannelType::Advertiser, chan.whit_init(), 0x555555, 0x8e89bed6).await.expect("Set params");
}

async fn switch_channel(lr2021: &mut Lr2021Stm32, chan: AdvChanRf, addr_seen: &AddrList, is_rx: bool) {
    let intr = lr2021.get_and_clear_irq().await.expect("GetIrqs");
    let stat = lr2021.get_ble_rx_stats_adv().await.expect("RX Stats");
    info!("[RX] Stats: RX={}, CRC ok={}, CRC err={}, Len err={}, Sync Fail={} | {}",
        stat.pkt_rx(), stat.crc_ok(), stat.crc_error(), stat.len_error(), stat.sync_fail(), intr);
    lr2021.clear_rx_stats().await.unwrap();
    lr2021.clear_rx_fifo().await.unwrap();
    if addr_seen.size() > 0 {
        info!("Addr.Seen: {}", addr_seen);
    }
    info!("[RX] Switching to channel {}",chan);
    lr2021.set_chip_mode(ChipMode::Fs).await.expect("SetFs");
    lr2021.set_rf(chan.freq()).await.expect("SetRF");
    // lr2021.wait_ready(embassy_time::Duration::from_millis(1)).await.expect("WaitReady Post SetRF");
    // set_ble(lr2021, chan).await;
    lr2021.set_ble_params(false, ChannelType::Advertiser, chan.whit_init(), 0x555555, 0x8e89bed6).await.expect("Set params");
    if is_rx {
        lr2021.set_rx(0xFFFFFFFF, true).await.expect("SetRx");
    }
}

async fn send_beacon(lr2021: &mut Lr2021Stm32) {
    let len = ADV_BEACON.len();
    // Create payload and send it to the TX FIFO
    lr2021.buffer_mut().copy_from_slice(&ADV_BEACON);
    lr2021.wr_tx_fifo(len).await.expect("FIFO write");
    info!("[TX] Sending beacon");
    lr2021.set_ble_tx(len as u8).await.expect("SetTx");
    // Listen for response for 10ms (unit ~ 30.50us)
    lr2021.set_rx(328, true).await.expect("SetRx");
}

async fn send_req(lr2021: &mut Lr2021Stm32, req_type: BleAdvType, addr: u64) {
    let len = 14;
    lr2021.buffer_mut()[0] = req_type as u8;
    lr2021.buffer_mut()[1] = 12;
    lr2021.buffer_mut()[2..8].copy_from_slice(&[0xa4, 0x63, 0xef, 0x8c, 0x89, 0xe6]);
    lr2021.buffer_mut()[8 ] = ((addr >> 40) & 0xFF) as u8;
    lr2021.buffer_mut()[9 ] = ((addr >> 32) & 0xFF) as u8;
    lr2021.buffer_mut()[10] = ((addr >> 24) & 0xFF) as u8;
    lr2021.buffer_mut()[11] = ((addr >> 16) & 0xFF) as u8;
    lr2021.buffer_mut()[12] = ((addr >>  8) & 0xFF) as u8;
    lr2021.buffer_mut()[13] = ( addr        & 0xFF) as u8;
    lr2021.wr_tx_fifo(len).await.expect("FIFO write");
    info!("[TX] Sending Scan request to {:06x}", addr);
    lr2021.set_ble_tx(len as u8).await.expect("SetTx");
}

async fn switch_mode(lr2021: &mut Lr2021Stm32, chan: AdvChanRf, is_rx: bool) {
    lr2021.set_chip_mode(ChipMode::Fs).await.expect("SetFs");
    if is_rx {
        lr2021.set_rx(0xFFFFFFFF, true).await.expect("SetRx");
        LED_TX_MODE.signal(LedMode::Off);
        LED_RX_MODE.signal(LedMode::BlinkSlow);
        info!(" -> Switched to RX (chan: {})", chan);
    } else {
        LED_TX_MODE.signal(LedMode::BlinkSlow);
        LED_RX_MODE.signal(LedMode::Off);
        info!(" -> Switching to FS: ready for TX on {}", chan);
    }
}


async fn read_pkt(lr2021: &mut Lr2021Stm32, intr: Intr) -> Option<BlePacketStatusRsp> {
    let lvl = lr2021.get_rx_fifo_lvl().await.expect("RxFifoLvl");
    let pkt_status = lr2021.get_ble_packet_status().await.expect("PktStatus");
    let nb_byte = pkt_status.pkt_len().min(128) as usize;
    if lvl == 0 && nb_byte != 0 {
        warn!("No data in fifo ({}) | {}", nb_byte, intr);
        return None;
    }

    lr2021.rd_rx_fifo(nb_byte).await.expect("RX FIFO Read");
    Some(pkt_status)
}
