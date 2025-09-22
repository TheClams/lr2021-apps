#![no_std]
#![no_main]

// LoRa Ranging demo
// The green led blinks when in responder mode, and is off in Initiator mode
// Single press in Initiator start a burst of 20 ranging exhcnage
// Single press in Responder show some stats
// Double press enable frequency hopping between each exchange

use defmt::*;
use embassy_stm32::{mode::Async, usart::Uart};
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

use core::fmt::Write;
use heapless::String;

use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};

use lr2021_apps::board::{BoardNucleoL476Rg, ButtonPressKind, LedMode, Lr2021Stm32};
use lr2021::{
    lora::{LoraBw, LoraModulationParams, Sf},
    radio::{PacketType, RampTime, RxBoost, RxPath},
    status::{Intr, IRQ_MASK_RNG_EXCH_VLD, IRQ_MASK_RNG_REQ_DIS, IRQ_MASK_RNG_RESP_DONE, IRQ_MASK_RNG_TIMEOUT, IRQ_MASK_TIMEOUT},
    system::{ChipMode, DioNum}
};

const BW : LoraBw = LoraBw::Bw125;
const SF : Sf = Sf::Sf10;
const NB_PKT : u8 = 16;

const ADDR_INI: u32 = 0xC0FECD05;
const ADDR_RSP: u32 = 0xC0FECD05;

const RF_START: u32 = 895_000_000;
const RF_STOP : u32 = 905_000_000;
const RF_STEP : u32 =   1_000_000;

#[derive(Debug, Clone, Copy, Format, PartialEq)]
enum RngMode {Burst, Hopping, Tracking}
#[allow(dead_code)]
impl RngMode {
    pub fn next(&self) -> RngMode {
        match self {
            RngMode::Burst => RngMode::Hopping,
            RngMode::Hopping => RngMode::Tracking,
            RngMode::Tracking => RngMode::Burst,
        }
    }
    pub fn is_burst(&self) -> bool{
        *self == RngMode::Burst
    }
    pub fn is_hopping(&self) -> bool{
        *self == RngMode::Hopping
    }
    pub fn is_tracking(&self) -> bool{
        *self == RngMode::Tracking
    }
}

struct State {
    /// Remaining packet inside the ranging burst
    pkt_rem: u8,
    /// Timeout counter
    to_cnt: u8,
    /// Hopping feature control
    mode : RngMode,
    /// Board role: initiator or responder
    initiator : bool,
    /// RF channel
    rf: u32,
    /// RSSI offset to apply on ranging result
    rssi_offset: i16,
}

impl State {
    /// Create Stated efault as responder
    pub fn new(rssi_offset: i16) -> Self {
        Self {
            pkt_rem: 0,
            to_cnt: 0,
            mode: RngMode::Burst,
            initiator: false,
            rf: RF_START,
            rssi_offset,
        }
    }

    /// Change role
    pub fn abort(&mut self) {
        self.pkt_rem = 0;
        self.to_cnt = 0;
        self.rf = RF_START;
    }

    /// Change role
    pub fn toggle_role(&mut self) {
        self.initiator = !self.initiator;
        let role = if self.initiator {"Initiator"} else {"Responder"};
        info!("Board role set to {}", role);
    }

    /// Switch to next ranging mode
    pub fn next_mode(&mut self) {
        self.mode = self.mode.next();
        info!("Mode {}", self.mode);
    }

    /// Increment RF
    /// Return true when wrapping
    pub fn hop(&mut self) -> bool {
        self.rf += RF_STEP;
        if self.rf > RF_STOP {
            self.rf = RF_START;
            true
        } else {
            false
        }
    }

}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Starting lora_ranging");

    let board = BoardNucleoL476Rg::init(&spawner).await;
    let mut lr2021 = board.lr2021;
    let mut irq = board.irq;
    let mut uart = board.uart;
    BoardNucleoL476Rg::led_green_set(LedMode::BlinkSlow);

    // Initialize transceiver for LoRa communication
    lr2021.set_rf(RF_START).await.expect("SetRF");
    lr2021.set_rx_path(RxPath::LfPath, RxBoost::Max).await.expect("Setting RX path to LF");
    lr2021.calib_fe(&[]).await.expect("Front-End calibration");
    lr2021.set_tx_params(22, RampTime::Ramp8u).await.expect("SetTxParams");

    match lr2021.get_status().await {
        Ok((status, intr)) => info!("Calibration Done: {} | {}", status, intr),
        Err(e) => warn!("Calibration Failed: {}", e),
    }

    let modulation = LoraModulationParams::basic(SF, BW);

    lr2021.set_packet_type(PacketType::Ranging).await.expect("Setting packet type");
    lr2021.patch_ranging_rf().await.expect("PatchRangingRf");
    lr2021.set_ranging_modulation(&modulation, false).await.expect("SetModulation");
    lr2021.set_ranging_dev_addr(ADDR_RSP, None).await.expect("SetDevAddr"); // Default role is responder
    lr2021.set_ranging_req_addr(ADDR_RSP).await.expect("SetReqAddr");
    lr2021.set_ranging_params(true, false, 12).await.expect("SetRangingParams");
    let delay = lr2021.get_ranging_base_delay(&modulation);
    lr2021.set_ranging_txrx_delay(delay-10).await.expect("SetRangingDelay"); // Value depends on SF, BW and PCB

    // Start RX continuous
    match lr2021.set_rx(0xFFFFFFFF, true).await {
        Ok(_) => info!("[RX] Searching Preamble"),
        Err(e) => error!("Fail while set_rx() : {}", e),
    }

    // Set DIO7 as IRQ for RX Done
    lr2021.set_dio_irq(DioNum::Dio7, Intr::new(IRQ_MASK_RNG_EXCH_VLD|IRQ_MASK_RNG_RESP_DONE|IRQ_MASK_RNG_REQ_DIS|IRQ_MASK_TIMEOUT|IRQ_MASK_RNG_TIMEOUT)).await.expect("Setting DIO7 as IRQ");

    // Wait for a button press for actions
    let mut button_press = BoardNucleoL476Rg::get_button_evt();

    let rssi_offset = lr2021.get_ranging_rssi_offset().await.expect("GetRngOffset");
    let mut state = State::new(rssi_offset);

    loop {
        match select(button_press.changed(), irq.wait_for_rising_edge()).await {
            Either::First(press) => {
                match press {
                    // Double press => Toggle Mode
                    ButtonPressKind::Double => {
                        state.next_mode();
                        let uart_msg = match state.mode {
                            RngMode::Burst    => [b'B', b'\n'],
                            RngMode::Hopping  => [b'H', b'\n'],
                            RngMode::Tracking => [b'T', b'\n'],
                        };
                        uart.write(&uart_msg).await.ok();
                    }
                    // Short press:
                    // If initiator and no burst is ongoing send packet and init burst size to NB_PKT
                    // Otherwise show stats and set remaiing burst to 0
                    ButtonPressKind::Short => {
                        if state.initiator && state.pkt_rem == 0 {
                            state.pkt_rem = NB_PKT;
                            send_pkt(&mut lr2021, &mut state).await;
                        } else {
                            state.pkt_rem = 0;
                            show_stats(&mut lr2021, state.initiator).await;
                        }
                    }
                    // Long press: switch role TX/RX
                    ButtonPressKind::Long => {
                        state.toggle_role();
                        let addr = if state.initiator {ADDR_INI} else {ADDR_RSP} ;
                        lr2021.set_ranging_modulation(&modulation, state.initiator).await.expect("SetModulation");
                        lr2021.set_ranging_dev_addr(addr, None).await.expect("SetDevAddr");
                        switch_mode(&mut lr2021, state.initiator).await;
                    }
                }
            }
            // Interrupt
            Either::Second(_) => {
                match lr2021.get_and_clear_irq().await {
                    Ok(intr) => {
                        // Interrupt handling
                        if intr.rng_resp_done() {
                            let fei = lr2021.get_lora_fei().await.expect("Rd Freq");
                            info!("Response Done : FEI = {}", fei);
                            BoardNucleoL476Rg::led_green_set(LedMode::Flash);
                        } else if intr.rng_req_dis() {
                            info!("Request discarded ! {}", intr);
                            BoardNucleoL476Rg::led_red_set(LedMode::Flash);
                       } else if intr.timeout() || intr.rng_timeout() {
                            if state.to_cnt==0 {
                                info!("Timeout ! {}", intr);
                            }
                            BoardNucleoL476Rg::led_red_set(LedMode::Flash);
                            state.to_cnt += 1;
                        } else if intr.rng_exch_vld()  {
                            state.to_cnt = 0;
                            show_ranging_meas(&mut lr2021, &mut uart, &state).await;
                        }
                        let exchg_done = intr.rng_req_vld() || intr.rng_req_dis() ||
                            intr.timeout() || intr.rng_exch_vld() || intr.rng_timeout() ;
                        // Change channel if hopping enabled
                        if state.mode.is_hopping() && exchg_done && state.to_cnt < 4 {
                            hop_rf(&mut lr2021, &mut state).await;
                        }

                        // After too many timeout just go back to initial RF and start continuous RX
                        if state.to_cnt == 4 && !state.mode.is_tracking() {
                            state.abort();
                            info!("Too many Timeout, back to {}MHz", state.rf/1000000);
                            lr2021.set_rf_ranging(state.rf).await.expect("SetRF");
                            if !state.initiator {
                                lr2021.set_rx(0xFFFFFFFF, true).await.expect("SetRx");
                            }
                        }
                        // On initiator side send a packet after 50ms is counter is still not null
                        // When last packet sent display somt stats
                        else if state.initiator && (intr.rng_exch_vld() || intr.rng_timeout()) {
                            if state.pkt_rem > 0 {
                                let delay = if state.mode.is_tracking() {
                                    if state.pkt_rem == NB_PKT - 4 {900} else {10}
                                } else {
                                    50
                                };
                                Timer::after_millis(delay).await;
                                send_pkt(&mut lr2021, &mut state).await;
                            } else {
                                show_stats(&mut lr2021, true).await;
                            }
                        }
                    }
                    Err(e) => {
                        let err = lr2021.get_errors().await;
                        error!("Error getting interrupt: {} | {}", e, err);
                    }
                }
            }
        }
    }
}

async fn hop_rf(lr2021: &mut Lr2021Stm32, state : &mut State) {
    state.hop();
    if !state.initiator {
        lr2021.set_chip_mode(ChipMode::Fs).await.expect("SetFs");
    }
    lr2021.set_rf_ranging(state.rf).await.expect("SetRF");
    // info!("Setting RF to {}MHz", state.rf/1000000);
    if !state.initiator {
        // TX send a packet roughly every 50ms, so set timeout RX after ~70us
        // Take extra margin to handle delay due to clock not being synchronized and debug print adding some delays
        // This allows to keep hopping and hopefully stays on the same RF as the initiator
        lr2021.set_rx(2100, true).await.expect("SetRx");
    }
}

async fn show_stats(lr2021: &mut Lr2021Stm32, is_initiator: bool) {
    let stats = lr2021.get_ranging_stats().await.expect("RX stats");
    lr2021.clear_rx_stats().await.expect("Clearing stats");
    if is_initiator {
        info!("[INI] Exchange={}/{}, Timeout={}",
            stats.exchange_valid(), stats.request_valid(), stats.timeout());
    } else {
        info!("[RSP] Response={}, Discard={}",
            stats.response_done(), stats.request_discarded());
    }
}

async fn send_pkt(lr2021: &mut Lr2021Stm32, state: &mut State) {
    lr2021.set_tx(0).await.expect("SetTx");
    if state.pkt_rem > 0 {
        state.pkt_rem -= 1;
        if state.mode.is_tracking() && state.pkt_rem < (NB_PKT - 4) {
            state.pkt_rem = NB_PKT;
        }
    }
}

async fn switch_mode(lr2021: &mut Lr2021Stm32, is_initiator: bool) {
    lr2021.set_chip_mode(ChipMode::Fs).await.expect("SetFs");
    if is_initiator {
        BoardNucleoL476Rg::led_red_set(LedMode::Off);
        BoardNucleoL476Rg::led_green_set(LedMode::Off);
    } else {
        lr2021.set_rx(0xFFFFFFFF, true).await.expect("SetRx");
        BoardNucleoL476Rg::led_red_set(LedMode::Off);
        BoardNucleoL476Rg::led_green_set(LedMode::BlinkSlow);
    }
}

async fn show_ranging_meas(lr2021: &mut Lr2021Stm32, uart: &mut Uart<'static, Async>, state: &State) {
    let result = lr2021.get_ranging_ext_result().await.expect("GetRangingResult");
    let rttof = (result.rng1() + result.rng2()) / 2;
    let doppler = result.rng2() - result.rng1();
    let dist_cm = (rttof * 150 * 100) / 4096; // Bandwidth 1MHz
    let rssi = state.rssi_offset + result.rssi1() as i16;
    // speed_kmh = (doppler - bias) * BW/(1<<sf) * c/2*3.6 / 4096 / RF
    info!("[RX] RF={}MHz {} | Dist = {} (raw = {}/{}), RSSI = {}dBm, Doppler = {}",
        state.rf/1000000, state.pkt_rem, dist_cm, result.rng1(), result.rng2(), rssi, doppler
    );
    // Uart:
    let mut s: String<32> = String::new();
    core::write!(&mut s, "{}/{}|{}\r\n", result.rng1(), result.rng2(), result.rssi1()).ok();
    uart.write(s.as_bytes()).await.ok();
}
