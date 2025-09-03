use defmt::{info, Format};
use embassy_executor::Spawner;
use embassy_stm32::{
    bind_interrupts,
    exti::ExtiInput,
    gpio::{Level, Output, Pull, Speed},
    mode::Async, spi::{Config as SpiConfig, Spi},
    time::Hertz,
    usart::{Config as UartConfig, Uart}
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal, watch::{Receiver, Watch}};
use embassy_time::{with_timeout, Duration, Timer};
use lr2021::{system::{DioFunc, DioNum, PullDrive}, BusyAsync, Lr2021};

bind_interrupts!(struct UartIrqs {
    USART2 => embassy_stm32::usart::InterruptHandler<embassy_stm32::peripherals::USART2>;
});

pub type Lr2021Stm32 = Lr2021<Output<'static>,SpiWrapper, BusyAsync<ExtiInput<'static>>>;
// pub type Lr2021Stm32 = Lr2021<Output<'static>,Spi<'static,Async>, BusyAsync<ExtiInput<'static>>>;

pub struct BoardNucleoL476Rg {
    pub lr2021: Lr2021Stm32,
    pub irq: ExtiInput<'static>,
    pub trigger_tx: Output<'static>,
    pub uart: Uart<'static, Async>
}

/// Generate event when the button is press with short (0) or long (1) duration
type WatchButtonPress = Watch<CriticalSectionRawMutex, ButtonPressKind, 3>;
type ButtonRcvr = Receiver<'static, CriticalSectionRawMutex, ButtonPressKind, 3>;
static BUTTON_PRESS: WatchButtonPress = Watch::new();
/// Led modes
static LED_RED_MODE: SignalLedMode = Signal::new();
static LED_GREEN_MODE: SignalLedMode = Signal::new();

impl BoardNucleoL476Rg {

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

    pub async fn init(spawner: &Spawner) -> BoardNucleoL476Rg {
        let mut config = embassy_stm32::Config::default();

        // Configure the system clock to run at 80MHz
        // STM32L476RG has a 16MHz HSI (High Speed Internal) oscillator
        // PLL formula: (HSI * PLLN) / (PLLM * PLLR) = (16MHz * 10) / (1 * 2) = 80MHz
        config.rcc.hsi = true;
        config.rcc.pll = Some(embassy_stm32::rcc::Pll {
            source: embassy_stm32::rcc::PllSource::HSI,     // Use HSI as PLL source
            prediv: embassy_stm32::rcc::PllPreDiv::DIV1,    // PLLM = 1
            mul: embassy_stm32::rcc::PllMul::MUL10,         // PLLN = 10
            divp: None,                                     // PLLP not used
            divq: None,                                     // PLLQ not used
            divr: Some(embassy_stm32::rcc::PllRDiv::DIV2),  // PLLR = 2
        });
        config.rcc.sys = embassy_stm32::rcc::Sysclk::PLL1_R;
        // config.rcc.ahb_pre = embassy_stm32::rcc::AHBPrescaler::DIV1;
        // config.rcc.apb1_pre = embassy_stm32::rcc::APBPrescaler::DIV1;
        // config.rcc.apb2_pre = embassy_stm32::rcc::APBPrescaler::DIV1;
        let p = embassy_stm32::init(config);

        // Leds & buttons
        let led_red = Output::new(p.PC1, Level::High, Speed::Low);
        let led_green = Output::new(p.PC0, Level::High, Speed::Low);
        let button = ExtiInput::new(p.PC13, p.EXTI13, Pull::Up);

        // Start the tasks
        spawner.spawn(blink(led_red, &LED_RED_MODE)).unwrap();
        spawner.spawn(blink(led_green, &LED_GREEN_MODE)).unwrap();
        spawner.spawn(user_intf(button, &BUTTON_PRESS)).unwrap();
        LED_RED_MODE.signal(LedMode::Off);
        LED_GREEN_MODE.signal(LedMode::Off);

        // Control pins
        let busy = ExtiInput::new(p.PB3, p.EXTI3, Pull::Up);
        let nreset = Output::new(p.PA0, Level::High, Speed::Low);

        let irq = ExtiInput::new(p.PB0, p.EXTI0, Pull::None); // DIO7
        let trigger_tx = Output::new(p.PA1, Level::Low, Speed::Medium); // DIO8

        // UART on Virtual Com: 115200bauds, 1 stop bit, no parity, no flow control
        let mut uart_config = UartConfig::default();
        uart_config.baudrate = 576000;
        let uart = Uart::new(p.USART2, p.PA3, p.PA2, UartIrqs, p.DMA1_CH7, p.DMA1_CH6, uart_config).unwrap();

        // SPI
        let mut spi_config = SpiConfig::default();
        spi_config.frequency = Hertz(12_000_000);
        let spi = SpiWrapper(Spi::new_blocking(p.SPI1, p.PA5, p.PA7, p.PA6, spi_config));
        // let spi = Spi::new(
        //     p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA1_CH3, p.DMA1_CH2, spi_config,
        // );
        let nss = Output::new(p.PA8, Level::High, Speed::VeryHigh);

        // Create driver and reset board
        let mut lr2021 = Lr2021::new(nreset, busy, spi, nss);
        lr2021.reset().await.expect("Resetting chip !");

        // Configure DIO8 as a TX Trigger
        lr2021.set_dio_function(DioNum::Dio8, DioFunc::TxTrigger, PullDrive::PullNone).await.expect("SetDioTxTrigger");

        // Check version
        let version = lr2021.get_version().await.expect("Reading firmware version !");
        info!("FW Version {}", version);
        BoardNucleoL476Rg{lr2021, irq, uart, trigger_tx}
    }

    pub fn get_button_evt() -> ButtonRcvr {
        BUTTON_PRESS.receiver().unwrap()
    }

    pub fn led_red_set(mode: LedMode) {
        LED_RED_MODE.signal(mode)
    }

    pub fn led_green_set(mode: LedMode) {
        LED_GREEN_MODE.signal(mode)
    }
}

/// Board role: TX or RX
#[derive(Debug, Clone, Copy, Format, PartialEq)]
pub enum BoardRole {
    Rx = 0,
    Tx = 1,
    TxAuto = 2,
}
impl BoardRole {
    pub fn toggle(&mut self) {
        *self = match self {
            BoardRole::Rx => BoardRole::Tx,
            _ => BoardRole::Rx,
        }
    }

    pub fn toggle_auto(&mut self) {
        *self = match self {
            BoardRole::Rx => BoardRole::TxAuto,
            _ => BoardRole::Rx,
        }
    }

    pub fn is_tx(&self) -> bool {
        matches!(self,BoardRole::Tx)
    }

    pub fn is_rx(&self) -> bool {
        matches!(self,BoardRole::Rx)
    }
}

impl From<u8> for BoardRole {
    fn from(value: u8) -> Self {
        match value {
            1 => BoardRole::Tx,
            _ => BoardRole::Rx,
        }
    }
}

#[derive(Debug, Clone, Copy, Format, PartialEq)]
pub enum ButtonPressKind {
    Short,
    Double,
    Long
}

impl ButtonPressKind {
    pub fn is_short(&self) -> bool {
       *self==ButtonPressKind::Short
    }
}

/// Task to handle the user interface:
///   - a long press change the board mode (TX or RX)
///   - a short press either send a packet (TX mode) or clear the RX stat (RX mode)
#[embassy_executor::task]
pub async fn user_intf(mut button: ExtiInput<'static>, watch: &'static WatchButtonPress) {
    let s = watch.sender();
    loop {
        button.wait_for_falling_edge().await;
        // Small wait to debounce button press
        Timer::after_millis(5).await;
        // Determine if this is a short or long press
        let k = match with_timeout(Duration::from_millis(500), button.wait_for_high()).await {
            // Short press -> check for another press shortly after
            Ok(_) => {
                match with_timeout(Duration::from_millis(150), button.wait_for_falling_edge()).await {
                    Ok(_) => ButtonPressKind::Double,
                    Err(_) => ButtonPressKind::Short,
                }
            }
            // Long press
            Err(_) => ButtonPressKind::Long,
        };
        s.send(k)
    }
}


/// Led Mode
#[derive(Debug, Clone, Copy, Format, PartialEq)]
pub enum LedMode {
    Off = 0,
    On  = 1,
    BlinkSlow = 2,
    BlinkFast = 3,
    Flash = 4,
}

impl LedMode {

    /// Blinking half period
    pub fn delay(&self) -> Duration {
        match self {
            LedMode::BlinkSlow => Duration::from_millis(500),
            LedMode::BlinkFast => Duration::from_millis(125),
            LedMode::Flash => Duration::from_millis(60),
            _ => Duration::from_ticks(0),
        }
    }

    /// Flag when LedMode is blinking
    pub fn is_blink(&self) -> bool {
        matches!(self, LedMode::BlinkSlow |LedMode::BlinkFast | LedMode::Flash)
    }

    /// Flag when LedMode is blinking
    pub fn is_burst(&self) -> bool {
        matches!(self, LedMode::Flash)
    }

    /// Flag when LedMode should be on
    pub fn is_on(&self) -> bool {
        matches!(self, LedMode::On)
    }
}

impl From<u8> for LedMode {
    fn from(value: u8) -> Self {
        match value {
            4 => LedMode::Flash,
            3 => LedMode::BlinkFast,
            2 => LedMode::BlinkSlow,
            1 => LedMode::On,
            _ => LedMode::Off,
        }
    }
}

pub type SignalLedMode = Signal<CriticalSectionRawMutex, LedMode>;

/// Task pool to control up to 3 leds (nucleo + 2 on the LR2021 module)
#[embassy_executor::task(pool_size = 2)]
pub async fn blink(mut led: Output<'static>, signal: &'static SignalLedMode) {
    let mut burst_cnt : u8 = 0;
    let mut prev_mode : LedMode = LedMode::BlinkSlow;
    let mut mode : LedMode = LedMode::BlinkSlow;
    loop {
        // Check if mode has changed
        if let Some(next_mode) = signal.try_take() {
            if !mode.is_burst() {
                prev_mode = mode;
            }
            mode = next_mode;
            // Init burst cnt on
            if mode.is_burst() {
                burst_cnt = 4;
            }
        }
        // Toggle led state after a delay if it should blink
        if mode.is_blink() {
            Timer::after(mode.delay()).await;
            led.toggle();
            if burst_cnt > 0 {
                burst_cnt -= 1;
                if burst_cnt == 0 {
                    mode = prev_mode;
                }
            }
        }
        // Set the state on/off and wait for change in mode
        else {
            burst_cnt = 0;
            if mode.is_on() {
                led.set_high();
            } else {
                led.set_low();
            }
            prev_mode = mode;
            mode = signal.wait().await;
            if mode.is_burst() {
                burst_cnt = 4;
            }
        }
    }
}


// Wrapper around blocking SPI to use the non-DMA SPI with the LR2021 driver
pub struct SpiWrapper(pub Spi<'static,embassy_stm32::mode::Blocking>);

impl embedded_hal_1::spi::ErrorType for SpiWrapper {
    type Error = embassy_stm32::spi::Error;
}

impl<W: embassy_stm32::spi::Word> embedded_hal_async::spi::SpiBus<W> for SpiWrapper {
    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn write(&mut self, words: &[W]) -> Result<(), Self::Error> {
        self.0.blocking_write(words)
    }

    async fn read(&mut self, words: &mut [W]) -> Result<(), Self::Error> {
        self.0.blocking_read(words)
    }

    async fn transfer(&mut self, read: &mut [W], write: &[W]) -> Result<(), Self::Error> {
        self.0.blocking_transfer(read, write)
    }

    async fn transfer_in_place(&mut self, words: &mut [W]) -> Result<(), Self::Error> {
        self.0.blocking_transfer_in_place(words)
    }
}

