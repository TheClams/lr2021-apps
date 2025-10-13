#![no_std]
#![no_main]

//! # Zigbee TX/RX demo application
//!

use defmt::*;
use {defmt_rtt as _, panic_probe as _};

use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};

use lr2021_apps::{board::{BoardNucleoL476Rg, ButtonPressKind, LedMode, Lr2021Stm32}, zigbee_utils::ZigbeeFrameType};
use lr2021_apps::zigbee_utils::{ZigbeeHdr, ZigbeeCmd};

use lr2021::{radio::{FallbackMode, PacketType, RampTime, RxBoost, RxPath}, system::{ChipMode, DioNum}};
use lr2021::status::{Intr, IRQ_MASK_RX_DONE, IRQ_MASK_TX_DONE};
// use lr2021::system::ChipMode;
use lr2021::zigbee::*;

#[derive(Debug, Clone, Copy, PartialEq, Format)]
pub enum AdvChanRf {Chan15, Chan20, Chan25, Chan26}

impl AdvChanRf {
    pub fn freq(&self) -> u32 {
        match self {
            AdvChanRf::Chan15 => 2_425_000_000,
            AdvChanRf::Chan20 => 2_450_000_000,
            AdvChanRf::Chan25 => 2_475_000_000,
            AdvChanRf::Chan26 => 2_480_000_000,
        }
    }

    pub fn next(&mut self) {
        *self = match self {
            AdvChanRf::Chan15 => AdvChanRf::Chan20,
            AdvChanRf::Chan20 => AdvChanRf::Chan25,
            AdvChanRf::Chan25 => AdvChanRf::Chan26,
            AdvChanRf::Chan26 => AdvChanRf::Chan15,
        }
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Starting zigbee_txrx");
    let board = BoardNucleoL476Rg::init(&spawner).await;
    let mut lr2021 = board.lr2021;
    let mut irq = board.irq;

    let mut chan = AdvChanRf::Chan15;

    // Initialize transceiver for LoRa communication
    lr2021.set_rf(chan.freq()).await.expect("Setting RF to 2.425 GHz");
    lr2021.set_rx_path(RxPath::HfPath, RxBoost::Off).await.expect("Setting RX path to LF");
    // lr2021.set_rf(2_400_000_000).await.expect("Setting RF to 2.4GHz");
    // lr2021.set_rx_path(RxPath::HfPath, 0).await.expect("Setting RX path to HF");
    lr2021.calib_fe(&[]).await.expect("Front-End calibration");
    match lr2021.get_status().await {
        Ok((status, intr)) => info!("Calibration Done: {} | {}", status, intr),
        Err(e) => warn!("Calibration Failed: {}", e),
    }

    lr2021.set_pa_hf().await.expect("Set PA HF");
    lr2021.set_tx_params(0, RampTime::Ramp8u).await.expect("SetTxParam");
    lr2021.set_fallback(FallbackMode::Fs).await.expect("Set fallback");

    // Configure Zigbee
    lr2021.set_packet_type(PacketType::Zigbee).await.expect("SetPkt");
    let params = ZigbeePacketParams::new(ZigbeeMode::Oqpsk250, 127, false);
    lr2021.set_zigbee_packet(&params).await.expect("SetPkt");
    lr2021.set_rx(0xFFFFFFFF, true).await.expect("SetRX");

    // Set DIO7 as IRQ for RX Done
    lr2021.set_dio_irq(DioNum::Dio7, Intr::new(IRQ_MASK_RX_DONE|IRQ_MASK_TX_DONE)).await.expect("Setting DIO7 as IRQ");

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
                        info!("ButtonPressKind::Double");
                    }
                    // Long press:
                    //  - When spy, switch channel
                    ButtonPressKind::Long => {
                        chan.next();
                        switch_channel(&mut lr2021, chan).await;
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
                if intr.rx_done() {
                    if intr.crc_error() {
                        BoardNucleoL476Rg::led_red_set(LedMode::Flash);
                        lr2021.clear_rx_fifo().await.expect("ClearFifo");
                    } else {
                        handle_rx_pkt(&mut lr2021).await;
                        BoardNucleoL476Rg::led_green_set(LedMode::Flash);
                    }
                }
                // On TxDone either go in scan or send another command (happens after an ack typically)
                if intr.tx_done() {
                }
            }
        }
    }
}

async fn switch_channel(lr2021: &mut Lr2021Stm32, chan: AdvChanRf) {
    let intr = lr2021.get_and_clear_irq().await.expect("GetIrqs");
    let stat = lr2021.get_zigbee_rx_stats().await.expect("RX Stats");
    info!("[RX] Stats: RX={},  CRC err={}, Len err={} | {}",
        stat.pkt_rx(), stat.crc_error(), stat.len_error(), intr);
    lr2021.set_chip_mode(ChipMode::Fs).await.expect("SetFs");
    lr2021.clear_rx_stats().await.unwrap();
    lr2021.clear_rx_fifo().await.unwrap();
    info!("[RX] Switching to {}",chan);
    lr2021.set_rf(chan.freq()).await.expect("SetRF");
    lr2021.set_rx(0xFFFFFFFF, true).await.expect("SetRx");
}

async fn show_and_clear_rx_stats(lr2021: &mut Lr2021Stm32) {
    let stats = lr2021.get_zigbee_rx_stats().await.expect("RX stats");
    info!("[RX] Clearing stats | RX={}, CRC Err={}, LenErr={}",
        stats.pkt_rx(),
        stats.crc_error(),
        stats.len_error()
    );
    lr2021.clear_rx_stats().await.unwrap();
}

async fn handle_rx_pkt(lr2021: &mut Lr2021Stm32) {
    let status = lr2021.get_zigbee_packet_status().await.expect("RX status");
    let nb_byte = status.pkt_len() as usize; // Make sure to not read more than the local buffer size
    lr2021.rd_rx_fifo(nb_byte).await.expect("RX FIFO Read");

    let mut bytes = lr2021.buffer().iter().take(nb_byte).copied();

    let lqi = status.lqi();
    let lqi_frac = (lqi&3) * 25;

    if let Some(hdr) = ZigbeeHdr::parse(&mut bytes) {
        // Suppose no IE ...
        let hdr_size = nb_byte - bytes.len();
        let pld = &lr2021.buffer()[hdr_size..nb_byte];
        info!("{} {:02x} | RSSI=-{}dBm, LQI={}.{}",
            hdr,
            pld,
            status.rssi_avg()>>1,
            lqi>>1, lqi_frac
        );
        if hdr.hdr_type==ZigbeeFrameType::Cmd {
            let cmd : ZigbeeCmd = bytes.next().unwrap().into();
            info!(" -> {}", cmd);
        }
    } else {
        info!("[Raw] {:02x} | RSSI=-{}dBm, LQI={}.{}",
            lr2021.buffer()[..nb_byte],
            status.rssi_avg()>>1,
            lqi>>1, lqi_frac
        );
    }

}
