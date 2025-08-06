use core::u32;

use defmt::Format;

use super::Lr2021Error;

/// Status sent at the beginning of each SPI command
/// B0 : 3:1 = Command status, 0 Interrupt pending
/// B1 : 7:4 Reset source, 2:0 Chip Mode
/// B2..B5 : Interrupts (32b)
#[derive(Default)]
pub struct Status([u8;6]);

/// Command status
#[derive(Format, PartialEq)]
pub enum CmdStatus {
    Fail = 0, // Last Command could not be executed
    PErr = 1, // Last command had invalid parameters or the OpCode is unknown
    Ok   = 2, // Last Command succeed
    Data = 3, // Last command succeed and now streaming data
    Unknown = 8, // Unknown status
}

/// Reset Source
#[derive(Format, PartialEq)]
pub enum ResetSrc {
    Cleared = 0,
    Analog = 1,
    External = 2,
    System = 3,
    Watchdog = 4,
    Iocd = 5,
    Rtc = 6,
    Unknown = 16, // Unknown Source
}

/// Chip Mode
#[derive(Format)]
pub enum ChipMode {
    Sleep = 0,
    Rc    = 1,
    Xosc  = 2,
    Fs    = 3,
    Rx    = 4,
    Tx    = 5,
    Unknown = 8, // Unknown Mode
}

impl Status {

    /// Create a status from up to 6 bytes
    pub fn from_slice(bytes: &[u8]) -> Status {
        let mut arr = [0;6];
        if bytes.len() > 6 {
            arr.copy_from_slice(&bytes[..6]);
        } else {
            arr[..bytes.len()].copy_from_slice(bytes);
        }
        Status(arr)
    }

    /// Update status: must be at most 6 bytes
    pub fn updt(&mut self, bytes: &[u8]) {
        self.0[..bytes.len()].copy_from_slice(bytes);
    }

    /// Update status: must be at most 6 bytes
    pub fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }

    /// Return the inner value as a slice (mostly for debug)
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Return Command status
    pub fn cmd(&self) -> CmdStatus {
        let bits_cmd = (self.0[0] >> 1) & 7;
        match bits_cmd {
            0 => CmdStatus::Fail,
            1 => CmdStatus::PErr,
            2 => CmdStatus::Ok,
            3 => CmdStatus::Data,
            _ => CmdStatus::Unknown,
        }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self.cmd(),CmdStatus::Ok | CmdStatus::Data)
    }

    /// Return true if an Interrupt is pending
    pub fn irq(&self) -> bool {
        (self.0[0] & 1) != 0
    }

    /// Return source of last reset
    pub fn reset_src(&self) -> ResetSrc {
        let bits_rst = (self.0[1] >> 4) & 15;
        match bits_rst {
            0 => ResetSrc::Cleared,
            1 => ResetSrc::Analog,
            2 => ResetSrc::External,
            3 => ResetSrc::System,
            4 => ResetSrc::Watchdog,
            5 => ResetSrc::Iocd,
            6 => ResetSrc::Rtc,
            _ => ResetSrc::Unknown
        }
    }

    /// Return source of last reset
    pub fn chip_mode(&self) -> ChipMode {
        let bits_mode = self.0[1] & 7;
        match bits_mode {
            0 => ChipMode::Sleep,
            1 => ChipMode::Rc,
            2 => ChipMode::Xosc,
            3 => ChipMode::Fs,
            4 => ChipMode::Rx,
            5 => ChipMode::Tx,
            _ => ChipMode::Unknown,
        }
    }

    /// Check command status and return Ok/Err
    pub fn check(&self) -> Result<(), Lr2021Error> {
        match self.cmd() {
            CmdStatus::Unknown => Err(Lr2021Error::Unknown),
            CmdStatus::Fail => Err(Lr2021Error::CmdFail),
            CmdStatus::PErr => Err(Lr2021Error::CmdErr),
            CmdStatus::Ok   |
            CmdStatus::Data => Ok(()),
        }
    }

    /// Return the interrupt status as u32
    pub fn intr(&self) -> u32 {
        let arr : [u8;4] = self.0[2..].try_into().unwrap();
        u32::from_be_bytes(arr)
    }

    /// Check if the interrupt status
    pub fn intr_match(&self, mask: u32) -> bool {
        self.intr() & mask != 0
    }

}


impl defmt::Format for Status {
    fn format(&self, fmt: defmt::Formatter) {
        match self.cmd() {
            CmdStatus::Fail    => defmt::write!(fmt, "Command failed !"),
            CmdStatus::PErr    => defmt::write!(fmt, "Illegal parameters"),
            CmdStatus::Unknown => defmt::write!(fmt, "Invalid status"),
            CmdStatus::Ok |
            CmdStatus::Data    => {
                defmt::write!(fmt, "Command succeded");
                if self.irq() {
                    defmt::write!(fmt, " | IRQ pending");
                }
                let rst = self.reset_src();
                if rst!=ResetSrc::Cleared {
                    defmt::write!(fmt, " | Reset from {}", rst);
                }
                defmt::write!(fmt, " | Chip in {}", self.chip_mode());
            }
        }
    }
}

pub const IRQ_MASK_RX_FIFO             : u32 = 0x00000001;
pub const IRQ_MASK_TX_FIFO             : u32 = 0x00000002;
pub const IRQ_MASK_RNG_REQ_VLD         : u32 = 0x00000004;
pub const IRQ_MASK_TX_TIMESTAMP        : u32 = 0x00000008;
pub const IRQ_MASK_RX_TIMESTAMP        : u32 = 0x00000010;
pub const IRQ_MASK_PREAMBLE_DETECTED   : u32 = 0x00000020;
pub const IRQ_MASK_LORA_HEADER_VALID   : u32 = 0x00000040;
pub const IRQ_MASK_CAD_DETECTED        : u32 = 0x00000080;

pub const IRQ_MASK_LORA_HDR_TIMESTAMP  : u32 = 0x00000100;
pub const IRQ_MASK_LORA_HEADER_ERR     : u32 = 0x00000200;
pub const IRQ_MASK_EOL                 : u32 = 0x00000400;
pub const IRQ_MASK_PA                  : u32 = 0x00000800;
pub const IRQ_MASK_LORA_TX_RX_HOP      : u32 = 0x00001000;
pub const IRQ_MASK_SYNC_FAIL           : u32 = 0x00002000;
pub const IRQ_MASK_LORA_SYMBOL_END     : u32 = 0x00004000;
pub const IRQ_MASK_LORA_TIMESTAMP_STAT : u32 = 0x00008000;

pub const IRQ_MASK_ERROR               : u32 = 0x00010000;
pub const IRQ_MASK_CMD                 : u32 = 0x00020000;
pub const IRQ_MASK_RX_DONE             : u32 = 0x00040000;
pub const IRQ_MASK_TX_DONE             : u32 = 0x00080000;
pub const IRQ_MASK_CAD_DONE            : u32 = 0x00100000;
pub const IRQ_MASK_TIMEOUT             : u32 = 0x00200000;
pub const IRQ_MASK_CRC_ERROR           : u32 = 0x00400000;
pub const IRQ_MASK_LEN_ERROR           : u32 = 0x00800000;

pub const IRQ_MASK_ADDR_ERROR          : u32 = 0x01000000;
pub const IRQ_MASK_FHSS                : u32 = 0x02000000;
pub const IRQ_MASK_INTER_PACKET1       : u32 = 0x04000000;
pub const IRQ_MASK_INTER_PACKET2       : u32 = 0x08000000;
pub const IRQ_MASK_RNG_RESP_DONE       : u32 = 0x10000000;
pub const IRQ_MASK_RNG_REQ_DIS         : u32 = 0x20000000;
pub const IRQ_MASK_RNG_EXCH_VLD        : u32 = 0x40000000;
pub const IRQ_MASK_RNG_TIMEOUT         : u32 = 0x80000000;