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
use embassy_stm32::mode::Async;
use embassy_stm32::spi::{Config, Spi};
use embassy_stm32::{
    exti::ExtiInput,
    gpio::{Level, Output, Pull, Speed},
    time::Hertz,
};
use embassy_sync::{signal::Signal, watch::Watch};

use lr2021_apps::board::{blink, user_intf, BoardRole, ButtonPressKind, LedMode, SignalLedMode, WatchButtonPress};
use lr2021::{
    flrc::*,
    radio::{FallbackMode, PaLfMode, PacketType, RampTime, RxPath},
    status::{Intr, IRQ_MASK_RX_DONE, IRQ_MASK_TX_DONE},
    system::ChipMode, BusyAsync, Lr2021, PulseShape
};

/// Generate event when the button is press with short (0) or long (1) duration
static BUTTON_PRESS: WatchButtonPress = Watch::new();
/// Led modes
static LED_TX_MODE: SignalLedMode = Signal::new();
static LED_RX_MODE: SignalLedMode = Signal::new();

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
    let p = embassy_stm32::init(Default::default());
    info!("Starting flrc_txrx");

    // Start tasks to blink the TX/RX leds
    // and handle the button press to
    let led_tx = Output::new(p.PC1, Level::High, Speed::Low);
    spawner.spawn(blink(led_tx, &LED_TX_MODE)).unwrap();
    LED_TX_MODE.signal(LedMode::Off);

    let led_rx = Output::new(p.PC0, Level::High, Speed::Low);
    spawner.spawn(blink(led_rx, &LED_RX_MODE)).unwrap();
    LED_RX_MODE.signal(LedMode::BlinkSlow);

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

    // Packet ID: correspond to first byte sent
    let mut pkt_id = 0_u8;
    let mut sw_sel = SwSel::Sw1;

    // Initialize transceiver for LoRa communication
    lr2021.set_rf(900_000_000).await.expect("Setting RF to 900MHz");
    lr2021.set_rx_path(RxPath::LfPath, 0).await.expect("Setting RX path to LF");
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

    // Set DIO7 as IRQ for RX Done
    lr2021.set_dio_irq(7, Intr::new(IRQ_MASK_TX_DONE|IRQ_MASK_RX_DONE)).await.expect("Setting DIO7 as IRQ");

    // Create data buffer to test the wr_fifo_from and rf_fifo_to APIs
    let mut data = [0;16];

    let mut role = BoardRole::Rx;

    // Wait for a button press for actions
    let mut button_press = BUTTON_PRESS.receiver().unwrap();
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
                    LED_TX_MODE.signal(LedMode::Flash);
                }
                if /*lvl > 0 && */intr.rx_done() {
                    show_rx_pkt(&mut lr2021, &mut data, intr).await;
                    if !intr.crc_error() {
                        LED_RX_MODE.signal(LedMode::Flash);
                    }
                }
            }
        }
    }
}

type Lr2021Stm32 = Lr2021<Output<'static>,Spi<'static, Async>, BusyAsync<ExtiInput<'static>>>;

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
        LED_TX_MODE.signal(LedMode::Off);
        LED_RX_MODE.signal(LedMode::BlinkSlow);
    } else {
        info!(" -> Switching to FS: ready for TX");
        LED_TX_MODE.signal(LedMode::BlinkSlow);
        LED_RX_MODE.signal(LedMode::Off);
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
