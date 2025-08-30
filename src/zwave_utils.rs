use defmt::{write, Format, Formatter};

#[derive(Debug, Clone, Copy, Format, PartialEq)]
/// ZWave Header type (4LSB of byte 5)
pub enum ZwaveHdrType {
    SingleCast = 1,
    Multicast = 2,
    Ack = 3,
    Routed = 8,
    Reserved = 15,
}

impl From<u8> for ZwaveHdrType {
    fn from(value: u8) -> Self {
        match value {
            1 => ZwaveHdrType::SingleCast ,
            2 => ZwaveHdrType::Multicast ,
            3 => ZwaveHdrType::Ack ,
            8 => ZwaveHdrType::Routed ,
            _ => ZwaveHdrType::Reserved ,
        }
    }
}

#[derive(Debug, Clone)]
/// ZWave Phy Header
pub struct ZwavePhyHdr {
    pub home_id: u32,
    pub hdr_type: ZwaveHdrType,
    pub src: u8,
    pub dst: u8,
    pub seq_num: u8,
    pub ack_req: bool,
}

impl ZwavePhyHdr {
    /// Extract Phy Header information form a byte stream
    pub fn parse(bytes: &[u8]) -> Option<Self>{
        let home_id : u32 =
            ((*bytes.first()? as u32) << 24) +
            ((*bytes.get(1)? as u32) << 16) +
            ((*bytes.get(2)? as u32) << 8) +
            (*bytes.get(3)? as u32);
        let src : u8 = *bytes.get(4)?;
        let dst : u8 = *bytes.get(8)?;
        let seq_num : u8 = *bytes.get(6)? & 0xF;
        let hdr_type: ZwaveHdrType = (*bytes.get(5)? & 0xF).into();
        let ack_req =  (*bytes.get(5)? & 0x40) != 0;   // Note: in channel config 3 the mask should be 0x80
        Some(Self {home_id, hdr_type, src, dst, seq_num, ack_req})
    }

    pub fn to_bytes(&self, len: u8) -> [u8; 9] {
        let fc0 = (self.hdr_type as u8) | if self.ack_req {0x40} else {0x00};
        [
            ((self.home_id>>24) & 0xFF) as u8,
            ((self.home_id>>16) & 0xFF) as u8,
            ((self.home_id>> 8) & 0xFF) as u8,
            ( self.home_id      & 0xFF) as u8,
            self.src,
            fc0,
            self.seq_num,
            len,
            self.dst
        ]
    }
}

impl Default for ZwavePhyHdr {
    fn default() -> Self {
        Self { home_id: 0, hdr_type: ZwaveHdrType::SingleCast, src: 0x00, dst: 0xFF, seq_num: 0, ack_req: false }
    }
}

impl Format for ZwavePhyHdr {
    fn format(&self, fmt: Formatter) {
        write!(fmt, "{:08x} | {} {:02x} -> {:02x} ({})",
            self.home_id, self.hdr_type, self.src, self.dst, self.seq_num);
        if self.ack_req {
            write!(fmt, " AckReq");
        }
    }
}


#[derive(Debug, Clone, Copy, Format, PartialEq)]
/// Command Frame identifier (when class is set to 1)
pub enum ZwaveCmd {
    Nop,
    Prot(ProtCmd),
    Security(SecurityCmd),
    Manufacturer(ManufacturerCmd),
    Version(VersionCmd),
    Invalid,
    Unknown,
    NonInterop,
}

#[derive(Debug, Clone, Copy, Format, PartialEq)]
/// Command Frame identifier (when class is set to 1)
pub enum ProtCmd {
    NodeInfo           = 0x01,
    ReqInfo            = 0x02,
    SetId              = 0x03,
    FindNodesInRange   = 0x04,
    GetNodesInRange    = 0x05,
    RangeInfo          = 0x06,
    CommandComplete    = 0x07,
    TransferPres       = 0x08,
    TransferNodeInfo   = 0x09,
    TransferRangeInfo  = 0x0A,
    TransferEnd        = 0x0B,
    AssignRoute        = 0x0C,
    NewNode            = 0x0D,
    NewRange           = 0x0E,
    TransferPrimary    = 0x0F,
    AutoStart          = 0x10,
    SucId              = 0x11,
    SetSuc             = 0x12,
    SetSucAck          = 0x13,
    AssignSucRoute     = 0x14,
    StaticRouteReq     = 0x15,
    Lost               = 0x16,
    AcceptLost         = 0x17,
    NopPower           = 0x18,
    ReserveId          = 0x19,
    NodesExist         = 0x1F,
    NodesExistReply    = 0x20,
    SetNwi             = 0x22,
    ExcludeReq         = 0x23,
    RoutePriority      = 0x24,
    SucRoutePriority   = 0x25,
    SmartStartNodeInfo = 0x26,
    SmartStartPrime    = 0x27,
    SmartStartReq      = 0x28,
    Unknown            = 0xFF,
}

impl From<u8> for ProtCmd {
    fn from(value: u8) -> Self {
        match value {
            0x01 => ProtCmd::NodeInfo,
            0x02 => ProtCmd::ReqInfo,
            0x03 => ProtCmd::SetId,
            0x04 => ProtCmd::FindNodesInRange,
            0x05 => ProtCmd::GetNodesInRange,
            0x06 => ProtCmd::RangeInfo,
            0x07 => ProtCmd::CommandComplete,
            0x08 => ProtCmd::TransferPres,
            0x09 => ProtCmd::TransferNodeInfo,
            0x0A => ProtCmd::TransferRangeInfo,
            0x0B => ProtCmd::TransferEnd,
            0x0C => ProtCmd::AssignRoute,
            0x0D => ProtCmd::NewNode,
            0x0E => ProtCmd::NewRange,
            0x0F => ProtCmd::TransferPrimary,
            0x10 => ProtCmd::AutoStart,
            0x11 => ProtCmd::SucId,
            0x12 => ProtCmd::SetSuc,
            0x13 => ProtCmd::SetSucAck,
            0x14 => ProtCmd::AssignSucRoute,
            0x15 => ProtCmd::StaticRouteReq,
            0x16 => ProtCmd::Lost,
            0x17 => ProtCmd::AcceptLost,
            0x18 => ProtCmd::NopPower,
            0x19 => ProtCmd::ReserveId,
            0x1F => ProtCmd::NodesExist,
            0x20 => ProtCmd::NodesExistReply,
            0x22 => ProtCmd::SetNwi,
            0x23 => ProtCmd::ExcludeReq,
            0x24 => ProtCmd::RoutePriority,
            0x25 => ProtCmd::SucRoutePriority,
            0x26 => ProtCmd::SmartStartNodeInfo,
            0x27 => ProtCmd::SmartStartPrime,
            0x28 => ProtCmd::SmartStartReq,
            _    => ProtCmd::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, Format, PartialEq)]
pub enum SecurityCmd {
    NonceGet = 0x40,
    NonceReport = 0x80,
    SchemeGet = 0x04,
    SchemeInherit = 0x08,
    SchemeReport = 0x05,
    Message = 0x81,
    MessageGet = 0xC1,
    Unknown = 0xFF,
}

impl From<u8> for SecurityCmd {
    fn from(value: u8) -> Self {
        match value {
            0x40 => SecurityCmd::NonceGet,
            0x80 => SecurityCmd::NonceReport,
            0x04 => SecurityCmd::SchemeGet,
            0x08 => SecurityCmd::SchemeInherit,
            0x05 => SecurityCmd::SchemeReport,
            0x81 => SecurityCmd::Message,
            0xC1 => SecurityCmd::MessageGet,
            _ => SecurityCmd::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, Format, PartialEq)]
pub enum ManufacturerCmd {
    Get = 0x04,
    Report = 0x05,
    Version = 0x01,
    Unknown = 0xFF,
}

impl From<u8> for ManufacturerCmd {
    fn from(value: u8) -> Self {
        match value {
            0x04 => ManufacturerCmd::Get,
            0x05 => ManufacturerCmd::Report,
            0x01 => ManufacturerCmd::Version,
            _    => ManufacturerCmd::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, Format, PartialEq)]
pub enum VersionCmd {
    Get = 0x11,
    Report = 0x12,
    Unknown = 0xFF,
}

impl From<u8> for VersionCmd {
    fn from(value: u8) -> Self {
        match value {
            0x11 => VersionCmd::Get,
            0x12 => VersionCmd::Report,
            _    => VersionCmd::Unknown,
        }
    }
}

impl ZwaveCmd {
    pub fn parse(bytes: &[u8]) -> ZwaveCmd {
        let Some(&class) = bytes.first() else {
            return ZwaveCmd::Invalid;
        };
        match class {
            0x00 => ZwaveCmd::Nop,
            0x01 => {
                let cmd = bytes.get(1).map(|&v| ProtCmd::from(v)).unwrap_or(ProtCmd::Unknown);
                ZwaveCmd::Prot(cmd)
            }
            0x98 => {
                let cmd = bytes.get(1).map(|&v| SecurityCmd::from(v)).unwrap_or(SecurityCmd::Unknown);
                ZwaveCmd::Security(cmd)
            }
            0xF0 => ZwaveCmd::NonInterop,
            0x72 => {
                let cmd = bytes.get(1).map(|&v| ManufacturerCmd::from(v)).unwrap_or(ManufacturerCmd::Unknown);
                ZwaveCmd::Manufacturer(cmd)
            }
            0x86 => {
                let cmd = bytes.get(1).map(|&v| VersionCmd::from(v)).unwrap_or(VersionCmd::Unknown);
                ZwaveCmd::Version(cmd)
            }
            _ => ZwaveCmd::Unknown,
        }

    }
}
