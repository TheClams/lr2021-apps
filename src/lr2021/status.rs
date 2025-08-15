use defmt::Format;

use super::Lr2021Error;

/// Status sent at the beginning of each SPI command
///  - 11:9 = Command status
///  -    8 Interrupt pending
///  -  7:4 Reset source
///  -  2:0 Chip Mode
#[derive(Default)]
pub struct Status(u16);

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
    Watchdog = 4,
    Iocd = 5,
    Rtc = 6,
    Unknown = 16, // Unknown Source
}

/// Chip Mode
#[derive(Format)]
pub enum ChipModeStatus {
    Sleep = 0,
    Rc    = 1,
    Xosc  = 2,
    Fs    = 3,
    Rx    = 4,
    Tx    = 5,
    Unknown = 8, // Unknown Mode
}

impl Status {

    /// Create a status from a slice of at least two elements
    pub fn from_slice(bytes: &[u8]) -> Status {
        let v = ((*bytes.first().unwrap_or(&0) as u16) << 8)
            | (*bytes.get(1).unwrap_or(&0) as u16);
        Status(v)
    }

    /// Return Command status
    pub fn cmd(&self) -> CmdStatus {
        let bits_cmd = (self.0 >> 9) & 7;
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
        (self.0 & 0x0100) != 0
    }

    /// Return source of last reset
    pub fn reset_src(&self) -> ResetSrc {
        let bits_rst = (self.0 >> 4) & 15;
        match bits_rst {
            0 => ResetSrc::Cleared,
            1 => ResetSrc::Analog,
            2 => ResetSrc::External,
            4 => ResetSrc::Watchdog,
            5 => ResetSrc::Iocd,
            6 => ResetSrc::Rtc,
            _ => ResetSrc::Unknown
        }
    }

    /// Return source of last reset
    pub fn chip_mode(&self) -> ChipModeStatus {
        let bits_mode = self.0 & 7;
        match bits_mode {
            0 => ChipModeStatus::Sleep,
            1 => ChipModeStatus::Rc,
            2 => ChipModeStatus::Xosc,
            3 => ChipModeStatus::Fs,
            4 => ChipModeStatus::Rx,
            5 => ChipModeStatus::Tx,
            _ => ChipModeStatus::Unknown,
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
pub const IRQ_MASK_HEADER_VALID        : u32 = 0x00000040;
pub const IRQ_MASK_CAD_DETECTED        : u32 = 0x00000080;

pub const IRQ_MASK_LORA_HDR_TIMESTAMP  : u32 = 0x00000100;
pub const IRQ_MASK_HEADER_ERR          : u32 = 0x00000200;
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

#[derive(Default, Clone, Copy)]
pub struct Intr(u32);

impl Intr {
    /// Create Interrupt status from a slice
    /// Handle gracefully case where slice is smaller than interrupt size
    /// (this happen)
    pub fn from_slice(bytes: &[u8]) -> Intr {
        let v = ((*bytes.first().unwrap_or(&0) as u32) << 24)
            | ((*bytes.get(1).unwrap_or(&0) as u32) << 16)
            | ((*bytes.get(2).unwrap_or(&0) as u32) <<  8)
            | (*bytes.get(3).unwrap_or(&0) as u32);
        Intr(v)
    }

    pub fn new(value: u32) -> Intr {
        Intr(value)
    }

    /// Return the interrupt status as u32
    pub fn value(&self) -> u32 {
        self.0
    }

    /// Check if the interrupt status
    pub fn intr_match(&self, mask: u32) -> bool {
        self.value() & mask != 0
    }

    pub fn none(&self) -> bool {
        self.0 == 0
    }

    pub fn rx_fifo(&self) -> bool {
        (self.0 & IRQ_MASK_RX_FIFO) != 0
    }
    pub fn tx_fifo(&self) -> bool {
        (self.0 & IRQ_MASK_TX_FIFO) != 0
    }
    pub fn rng_req_vld(&self) -> bool {
        (self.0 & IRQ_MASK_RNG_REQ_VLD) != 0
    }
    pub fn tx_timestamp(&self) -> bool {
        (self.0 & IRQ_MASK_TX_TIMESTAMP) != 0
    }
    pub fn rx_timestamp(&self) -> bool {
        (self.0 & IRQ_MASK_RX_TIMESTAMP) != 0
    }
    pub fn preamble_detected(&self) -> bool {
        (self.0 & IRQ_MASK_PREAMBLE_DETECTED) != 0
    }
    pub fn header_valid(&self) -> bool {
        (self.0 & IRQ_MASK_HEADER_VALID) != 0
    }
    pub fn cad_detected(&self) -> bool {
        (self.0 & IRQ_MASK_CAD_DETECTED) != 0
    }
    pub fn lora_hdr_timestamp(&self) -> bool {
        (self.0 & IRQ_MASK_LORA_HDR_TIMESTAMP) != 0
    }
    pub fn header_err(&self) -> bool {
        (self.0 & IRQ_MASK_HEADER_ERR) != 0
    }
    pub fn eol(&self) -> bool {
        (self.0 & IRQ_MASK_EOL) != 0
    }
    pub fn pa(&self) -> bool {
        (self.0 & IRQ_MASK_PA) != 0
    }
    pub fn lora_tx_rx_hop(&self) -> bool {
        (self.0 & IRQ_MASK_LORA_TX_RX_HOP) != 0
    }
    pub fn sync_fail(&self) -> bool {
        (self.0 & IRQ_MASK_SYNC_FAIL) != 0
    }
    pub fn lora_symbol_end(&self) -> bool {
        (self.0 & IRQ_MASK_LORA_SYMBOL_END) != 0
    }
    pub fn lora_timestamp_stat(&self) -> bool {
        (self.0 & IRQ_MASK_LORA_TIMESTAMP_STAT) != 0
    }
    pub fn error(&self) -> bool {
        (self.0 & IRQ_MASK_ERROR) != 0
    }
    pub fn cmd(&self) -> bool {
        (self.0 & IRQ_MASK_CMD) != 0
    }
    pub fn rx_done(&self) -> bool {
        (self.0 & IRQ_MASK_RX_DONE) != 0
    }
    pub fn tx_done(&self) -> bool {
        (self.0 & IRQ_MASK_TX_DONE) != 0
    }
    pub fn cad_done(&self) -> bool {
        (self.0 & IRQ_MASK_CAD_DONE) != 0
    }
    pub fn timeout(&self) -> bool {
        (self.0 & IRQ_MASK_TIMEOUT) != 0
    }
    pub fn crc_error(&self) -> bool {
        (self.0 & IRQ_MASK_CRC_ERROR) != 0
    }
    pub fn len_error(&self) -> bool {
        (self.0 & IRQ_MASK_LEN_ERROR) != 0
    }
    pub fn addr_error(&self) -> bool {
        (self.0 & IRQ_MASK_ADDR_ERROR) != 0
    }
    pub fn fhss(&self) -> bool {
        (self.0 & IRQ_MASK_FHSS) != 0
    }
    pub fn inter_packet1(&self) -> bool {
        (self.0 & IRQ_MASK_INTER_PACKET1) != 0
    }
    pub fn inter_packet2(&self) -> bool {
        (self.0 & IRQ_MASK_INTER_PACKET2) != 0
    }
    pub fn rng_resp_done(&self) -> bool {
        (self.0 & IRQ_MASK_RNG_RESP_DONE) != 0
    }
    pub fn rng_req_dis(&self) -> bool {
        (self.0 & IRQ_MASK_RNG_REQ_DIS) != 0
    }
    pub fn rng_exch_vld(&self) -> bool {
        (self.0 & IRQ_MASK_RNG_EXCH_VLD) != 0
    }
    pub fn rng_timeout(&self) -> bool {
        (self.0 & IRQ_MASK_RNG_TIMEOUT) != 0
    }

}

impl Format for Intr {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(f, "Intr: ");
        if self.none() {
            defmt::write!(f, "None");
            return;
        }
        if self.error()               {defmt::write!(f, "Error ")};
        if self.cmd()                 {defmt::write!(f, "CmdError ")};
        if self.rx_fifo()             {defmt::write!(f, "FifoRx ")};
        if self.tx_fifo()             {defmt::write!(f, "FifoTx ")};
        if self.eol()                 {defmt::write!(f, "EndOfLife ")};
        if self.pa()                  {defmt::write!(f, "PowerAmplifier ")};
        if self.preamble_detected()   {defmt::write!(f, "PreambleDetected ")};
        if self.sync_fail()           {defmt::write!(f, "SyncFail ")};
        if self.cad_detected()        {defmt::write!(f, "CadDetected ")};
        if self.timeout()             {defmt::write!(f, "Timeout ")};
        if self.crc_error()           {defmt::write!(f, "CrcError ")};
        if self.len_error()           {defmt::write!(f, "LenError ")};
        if self.addr_error()          {defmt::write!(f, "AddrError ")};
        if self.header_valid()        {defmt::write!(f, "HeaderValid ")};
        if self.header_err()          {defmt::write!(f, "HeaderError ")};
        if self.lora_tx_rx_hop()      {defmt::write!(f, "LoraTxRxHop ")};
        if self.lora_symbol_end()     {defmt::write!(f, "LoraSymbolEnd ")};
        if self.rx_done()             {defmt::write!(f, "RxDone ")};
        if self.tx_done()             {defmt::write!(f, "TxDone ")};
        if self.cad_done()            {defmt::write!(f, "CadDone ")};
        if self.tx_timestamp()        {defmt::write!(f, "TimestampTx ")};
        if self.rx_timestamp()        {defmt::write!(f, "TimestampRx ")};
        if self.lora_hdr_timestamp()  {defmt::write!(f, "TimestampLoraHeader ")};
        if self.lora_timestamp_stat() {defmt::write!(f, "TimestampLoraStat ")};
        if self.fhss()                {defmt::write!(f, "FHSS ")};
        if self.inter_packet1()       {defmt::write!(f, "InterPacket1 ")};
        if self.inter_packet2()       {defmt::write!(f, "InterPacket2 ")};
        if self.rng_resp_done()       {defmt::write!(f, "RangingRespDone ")};
        if self.rng_req_vld()         {defmt::write!(f, "RangingReqValid ")};
        if self.rng_req_dis()         {defmt::write!(f, "RangingReqDis ")};
        if self.rng_exch_vld()        {defmt::write!(f, "RangingExchValid ")};
        if self.rng_timeout()         {defmt::write!(f, "RangingTimeout")};
    }
}