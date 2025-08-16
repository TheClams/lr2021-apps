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
    gpio::{Input, Level, Output, Pull, Speed},
    time::Hertz,
};
use embassy_sync::{signal::Signal, watch::Watch};

use lr2021_apps::{board::{blink, user_intf, BoardRole, ButtonPressKind, LedMode, SignalLedMode, WatchButtonPress}, lr2021::{
    lora::{HeaderType, Ldro, LoraBw, LoraCr, Sf}, radio::{PacketType, RampTime, RxPath}, status::{Intr, IRQ_MASK_RX_DONE}, system::ChipMode, BusyBlocking, Lr2021
}};

/// Generate event when the button is press with short (0) or long (1) duration
static BUTTON_PRESS: WatchButtonPress = Watch::new();
/// Led modes
static LED_TX_MODE: SignalLedMode = Signal::new();
static LED_RX_MODE: SignalLedMode = Signal::new();

const PLD_SIZE : u8 = 10;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());
    info!("Starting lora_txrx");

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

    // Pin mapping
    // Name  | Connector | Nucleo
    // NRST  | CN8 A0    | PA0
    // SCK   | CN5 D13   | PA5
    // MISO  | CN5 D12   | PA6
    // MOSI  | CN5 D11   | PA7
    // NSS   | CN9 D7    | PA8
    // BUSY  | CN9 D3    | PB3
    // DIO7  | CN8 A3    | PB0
    // DIO8  | CN8 A1    | PA1
    // DIO9  | CN9 D5    | PB4
    // DIO10 | CN9 D4    | PB5
    // DIO11 | CN5 D8    | PA9
    // RFSW0 | CN9 D0    | PA3
    // RXSW1 | CN9 D1    | PA2
    // LEDTX | CN8 A5    | PC0
    // LEDRX | CN8 A4    | PC1

    // Control pins
    let busy = Input::new(p.PB3, Pull::Up);
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
    let mut lr2021 = Lr2021::new_blocking(nreset, busy, spi, nss);
    lr2021.reset().await.expect("Resetting chip !");

    // Check version
    let version = lr2021.get_version().await.expect("Reading firmware version !");
    info!("FW Version {}", version);

    // Packet ID: correspond to first byte sent
    let mut pkt_id = 0_u8;

    // Initialize transceiver for LoRa communication
    // 901MHz, 0dbM, SF5 BW1000, CR 4/5
    lr2021.set_rf(901_000_000).await.expect("Setting RF to 901MHz");
    lr2021.set_rx_path(RxPath::LfPath, 0).await.expect("Setting RX path to LF");
    lr2021.calib_fe(&[]).await.expect("Front-End calibration");

    match lr2021.get_status().await {
        Ok((status, intr)) => info!("Calibration Done: {} | {}", status, intr),
        Err(e) => warn!("Calibration Failed: {}", e),
    }

    lr2021.set_packet_type(PacketType::Lora).await.expect("Setting packet type");
    lr2021.set_lora_modulation(Sf::Sf5, LoraBw::Bw1000, LoraCr::Cr1Ham45Si, Ldro::Off).await.expect("Setting packet type");
    // Packet Preamble 8 Symbols, 10 Byte payload, Explicit header with CRC and up-chirp
    lr2021.set_lora_packet(8, PLD_SIZE, HeaderType::Explicit, true, false).await.expect("Setting packet parameters");
    lr2021.set_tx_params(0, RampTime::Ramp8u).await.expect("Setting TX parameters");

    // Start RX continuous
    match lr2021.set_rx(0xFFFFFFFF, true).await {
        Ok(_) => info!("[RX] Searching Preamble"),
        Err(e) => error!("Fail while set_rx() : {}", e),
    }

    // Set DIO9 as IRQ for RX Done
    lr2021.set_dio_irq(7, Intr::new(IRQ_MASK_RX_DONE)).await.expect("Setting DIO7 as IRQ");

    // Create data buffer
    let mut data = [0;16];

    // Wait for a button press for actions
    let mut button_press = BUTTON_PRESS.receiver().unwrap();

    let mut role = BoardRole::Rx;
    loop {
        match select(button_press.changed(), irq.wait_for_rising_edge()).await {
            Either::First(press) => {
                match (press, role) {
                    // Short press in RX => clear stats
                    (ButtonPressKind::Short, BoardRole::Rx) => show_and_clear_rx_stats(&mut lr2021).await,
                    // Short press in TX => send a packet
                    (ButtonPressKind::Short, BoardRole::Tx) => send_pkt(&mut lr2021, &mut pkt_id, &mut data).await,
                    // Long press: switch role TX/RX
                    (ButtonPressKind::Long, _) => {
                        role.toggle();
                        switch_mode(&mut lr2021, role.is_rx()).await;
                    }
                    (n, r) => warn!("{} in role {} not implemented !", n, r),
                }
            }
            // RX Interrupt
            Either::Second(_) => show_rx_pkt(&mut lr2021, &mut data).await,
        }
    }
}

type Lr2021Stm32 = Lr2021<Output<'static>,Spi<'static, Async>,BusyBlocking<Input<'static>>>;

async fn show_and_clear_rx_stats(lr2021: &mut Lr2021Stm32) {
    let stats = lr2021.get_lora_rx_stats().await.expect("RX stats");
    info!("[RX] Clearing stats | RX={}, CRC Err={}, HdrErr={}, FalseSync={}",
        stats.pkt_rx(),
        stats.crc_error(),
        stats.header_error(),
        stats.false_sync(),
    );
}

async fn send_pkt(lr2021: &mut Lr2021Stm32, pkt_id: &mut u8, data: &mut [u8]) {
    info!("[TX] Sending packet {}", *pkt_id);
    // Create payload and send it to the TX FIFO
    for (i,d) in data.iter_mut().take(PLD_SIZE.into()).enumerate() {
        *d = pkt_id.wrapping_add(i as u8);
    }
    lr2021.wr_tx_fifo(&mut data[..PLD_SIZE.into()]).await.expect("FIFO write");
    lr2021.set_tx(0).await.expect("SetTx");
    *pkt_id += 1;
}

async fn switch_mode(lr2021: &mut Lr2021Stm32, is_rx: bool) {
    lr2021.set_chip_mode(ChipMode::Fs).await.expect("SetFs");
    if is_rx {
        lr2021.set_rx(0xFFFFFFFF, true).await.expect("SetRx");
        LED_TX_MODE.signal(LedMode::Off);
        LED_RX_MODE.signal(LedMode::BlinkSlow);
        info!(" -> Switched to RX");
    } else {
        lr2021.set_lora_packet(8, PLD_SIZE, HeaderType::Explicit, true, false).await.expect("Setting packet parameters");
        LED_TX_MODE.signal(LedMode::BlinkSlow);
        LED_RX_MODE.signal(LedMode::Off);
        info!(" -> Switching to FS: ready for TX");
    }
}

async fn show_rx_pkt(lr2021: &mut Lr2021Stm32, data: &mut [u8]) {
    let pkt_len = lr2021.get_rx_pkt_len().await.expect("RX Fifo level");
    let nb_byte = pkt_len.min(16) as usize; // Make sure to not read more than the local buffer size
    lr2021.rd_rx_fifo(&mut data[..nb_byte]).await.expect("RX FIFO Read");
    let intr = lr2021.get_and_clear_irq().await.expect("Getting intr");
    let status = lr2021.get_lora_packet_status_adv().await.expect("RX status");
    let snr = status.snr_pkt();
    let snr_frac = (snr&3) * 25;
    info!("[RX] Payload = {:02x} | intr={:08x} | RSSI=-{}dBm, SNR={}.{:02}, FEI={}",
        data[..nb_byte],
        intr.value(),
        status.rssi_pkt()>>1,
        snr>>2, snr_frac,
        status.freq_offset()
    );
}
