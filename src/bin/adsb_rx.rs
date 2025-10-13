#![no_std]
#![no_main]

// ADS-B demo
// Long press on user button run an RSSI estimation and update the detection threshold
// Double press alternate between the two ADS-B channel 1090 and 978MHz
// Short press display RX stats

const RSSI_MARGIN : i16 = 15; // Margin in dB above noise level for detection

use defmt::*;
use {defmt_rtt as _, panic_probe as _};

use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};

use core::fmt::Write;
use heapless::String;

use lr2021_apps::board::{BoardNucleoL476Rg, ButtonPressKind, LedMode, Lr2021Stm32};
use lr2021::{
    ook::*,
    radio::{RxBoost, RxPath},
    status::{Intr, IRQ_MASK_RX_DONE},
    system::{ChipMode, DioNum}
};

#[derive(Debug, Clone, Copy, PartialEq, Format)]
pub enum AdsbChan {HighLevel, LowLevel}

impl AdsbChan {
    pub fn freq(&self) -> u32 {
        match self {
            AdsbChan::HighLevel => 1_090_000_000,
            AdsbChan::LowLevel  =>   978_000_000,
        }
    }
    pub fn next(&mut self) {
        *self = match self {
            AdsbChan::HighLevel => AdsbChan::LowLevel,
            AdsbChan::LowLevel => AdsbChan::HighLevel,
        }
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Starting adsb_rx");

    let board = BoardNucleoL476Rg::init(&spawner).await;
    let mut lr2021 = board.lr2021;
    let mut irq = board.irq;
    let mut uart = board.uart;

    // Select Out-of-band channel to avoid immediately picking BLE traffic and allow board-to-board communication
    let mut chan = AdsbChan::HighLevel;

    // Wait for a button press for actions
    let mut button_press = BoardNucleoL476Rg::get_button_evt();

    // Initialize transceiver for ADS-B reception with max boost
    lr2021.set_rf(chan.freq()).await.expect("SetRF");
    lr2021.set_rx_path(RxPath::LfPath, RxBoost::Max).await.expect("SetRxPath");
    lr2021.calib_fe(&[]).await.expect("Front-End calibration");

    match lr2021.get_status().await {
        Ok((status, intr)) => info!("Calibration Done: {} | {}", status, intr),
        Err(e) => warn!("Calibration Failed: {}", e),
    }

    // Configure demodulator
    lr2021.set_ook_adsb().await.expect("SetOokAdsb");
    lr2021.force_crc_out().await.expect("CrcOut"); // Output CRC even if already checked, mainly for debug

    // Setup radio to max gain (saturation unlikely in ADS-B and AGC might induce packet loss)
    lr2021.set_rx_gain(13).await.expect("SetGain");
    lr2021.set_rx_continous().await.expect("SetRX");

    // Adjust the detection threshold to avoid false detection due to high noise level
    auto_thr(&mut lr2021).await;

    // Set DIO7 as IRQ for TX/RX Done
    lr2021.set_dio_irq(DioNum::Dio7, Intr::new(IRQ_MASK_RX_DONE)).await.expect("Setting DIO7 as IRQ");

    loop {
        match select(button_press.changed(), irq.wait_for_high()).await {
            Either::First(press) => {
                match press {
                    // Short Press: show stats and clean it
                    ButtonPressKind::Short => {
                        let stats = lr2021.get_ook_rx_stats().await.expect("RxStats");
                        lr2021.clear_rx_stats().await.expect("ClearStats");
                        info!("RX Stats: nb={}, err={}", stats.pkt_rx(), stats.crc_error());
                    }
                    // Long press: measure RSSI and adjust detection threshold
                    ButtonPressKind::Long => {
                        auto_thr(&mut lr2021).await;
                    }
                    // Double press => change channel
                    ButtonPressKind::Double => {
                        chan.next();
                        info!("Switching to {}", chan);
                        lr2021.set_chip_mode(ChipMode::Fs).await.expect("SetFs");
                        lr2021.set_rf(chan.freq()).await.expect("SetRF");
                        lr2021.set_rx_continous().await.expect("SetRx");
                        auto_thr(&mut lr2021).await;
                    }
                }
            }
            // Interrupt
            Either::Second(_) => {
                // Clear all IRQs
                let intr = lr2021.get_and_clear_irq().await.expect("GetIrqs");
                // Make sure the FIFO contains data
                let lvl = lr2021.get_rx_fifo_lvl().await.expect("RxFifoLvl");
                if intr.crc_error() {
                    lr2021.clear_rx_fifo().await.unwrap();
                    BoardNucleoL476Rg::led_red_set(LedMode::Flash);
                    // let pkt_status = lr2021.get_ook_packet_status().await.expect("PktStatus");
                    // let rssi_dbm = pkt_status.rssi_avg()>>1;
                    // warn!("CRC KO | -{}dBm | Fifo {}", rssi_dbm, lvl);
                }
                else if lvl > 0 && intr.rx_done() {
                    if let Some(pkt_status) = read_pkt(&mut lr2021, intr).await {
                        let nb_byte = pkt_status.pkt_len().min(14) as usize;
                        let pkt = &lr2021.buffer()[..nb_byte];
                        let rssi_dbm = pkt_status.rssi_high()>>1;
                        BoardNucleoL476Rg::led_green_set(LedMode::Flash);
                        info!("CRC OK: {=[u8]:02x} | -{}dBm ", pkt, rssi_dbm);
                        let mut s: String<128> = String::new();
                        for b in pkt {
                            core::write!(&mut s, "{b:02x}").ok();
                        }
                        core::write!(&mut s, " | -{}dBm\r\n", rssi_dbm).ok();
                        uart.write(s.as_bytes()).await.ok();
                    }
                }
            }
        }
    }
}

async fn read_pkt(lr2021: &mut Lr2021Stm32, intr: Intr) -> Option<OokPacketStatusRsp> {
    let lvl = lr2021.get_rx_fifo_lvl().await.expect("RxFifoLvl");
    let pkt_status = lr2021.get_ook_packet_status().await.expect("PktStatus");
    let nb_byte = pkt_status.pkt_len().min(128) as usize;
    if lvl == 0 && nb_byte != 0 {
        warn!("No data in fifo ({}) | {}", nb_byte, intr);
        return None;
    }
    lr2021.rd_rx_fifo(nb_byte).await.expect("RX FIFO Read");
    Some(pkt_status)
}

/// Automatically adjust the OOK detectio threshold based on RSSI measurement
async fn auto_thr(lr2021: &mut Lr2021Stm32) {
    lr2021.set_chip_mode(ChipMode::Fs).await.expect("SetFS");
    let cca_info = lr2021.set_and_get_cca(320, None).await.expect("SetCCA");
    let rssi_dbm = - ((cca_info.rssi_min() >> 1) as i16);
    // Estimate threshold
    let thr = 64 + RSSI_MARGIN + rssi_dbm;
    lr2021.set_ook_thr(thr as i8).await.expect("SetOokThr");
    // Restart reception in continuous mode
    lr2021.set_rx_continous().await.expect("SetRX");
    info!("RSSI = {}dBm -> thr = {}", rssi_dbm, thr);
}
