use defmt::Format;
use embassy_stm32::{exti::ExtiInput, gpio::Output};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, watch::Watch, signal::Signal};
use embassy_time::{with_timeout, Duration, Timer};


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

pub type WatchButtonPress = Watch<CriticalSectionRawMutex, ButtonPressKind, 3>;

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
