use defmt::{write, Format, Formatter};

#[derive(Debug, Clone, Copy, Format, PartialEq)]
/// Zigbee Header type (4LSB of byte 5)
pub enum ZigbeeFrameType {
    Beacon = 0,
    Data = 1,
    Ack = 2,
    Cmd = 3,
    Multi = 5,
    Frak = 6,
    Reserved = 7,
}

impl From<u8> for ZigbeeFrameType {
    fn from(value: u8) -> Self {
        match value&7 {
            0 => ZigbeeFrameType::Beacon ,
            1 => ZigbeeFrameType::Data ,
            2 => ZigbeeFrameType::Ack ,
            3 => ZigbeeFrameType::Cmd ,
            5 => ZigbeeFrameType::Multi ,
            6 => ZigbeeFrameType::Frak ,
            _ => ZigbeeFrameType::Reserved ,
        }
    }
}

#[derive(Debug, Clone, Copy, Format, PartialEq)]
pub enum AddrMode {
    Absent, Short, Long
}

impl AddrMode {
    pub fn from_byte(value: u8) -> Option<Self> {
        match value&3 {
            0 => Some(AddrMode::Absent),
            2 => Some(AddrMode::Short),
            3 => Some(AddrMode::Long),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Format, PartialEq)]
pub enum NodeId {
    Absent, Short(u16), Long(u64)
}

#[derive(Debug, Clone, Copy, Format, PartialEq)]
pub enum PanId {
    Absent, Short(u16)
}

#[derive(Debug, Clone)]
pub struct Addr {
    pub pan_id: PanId,
    pub node_id: NodeId
}

impl Format for Addr {
    fn format(&self, fmt: Formatter) {
        match self.pan_id {
            PanId::Absent => {}
            PanId::Short(id) => write!(fmt, "{:04x}:",id),
        }
        match self.node_id {
            NodeId::Absent => write!(fmt, "N/A"),
            NodeId::Short(id) => write!(fmt, "{:04x}",id),
            NodeId::Long(id) => write!(fmt, "{:016x}",id),
        }
    }
}

impl Addr {
    pub fn from_bytes(mode: AddrMode, pan_en: bool, iter: &mut impl Iterator<Item = u8>) -> Option<Self> {
        let pan_id = if pan_en && mode != AddrMode::Absent {
            let id : u16 = iter.take(2).enumerate().fold(0u16, |id, (i,b)| id + ((b as u16) << (8*i))) ;
            PanId::Short(id)
        } else {
            PanId::Absent
        };
        let node_id = match mode {
            AddrMode::Absent => NodeId::Absent,
            AddrMode::Short  => {
                let id : u16 = iter.take(2).enumerate().fold(0u16, |id, (i,b)| id + ((b as u16) << (8*i))) ;
                NodeId::Short(id)
            }
            AddrMode::Long   => {
                let id : u64 = iter.take(8).enumerate().fold(0u64, |id, (i,b)| id + ((b as u64) << (8*i))) ;
                NodeId::Long(id)
            }
        };
        Some(Self {pan_id, node_id})
    }
}

#[derive(Debug, Clone, Copy, Format, PartialEq)]
pub enum ZigbeeVersion {
    V0 = 0,
    V1 = 1,
    V2 = 2,
    Invalid
}

impl From<u8> for ZigbeeVersion {
    fn from(value: u8) -> Self {
        match value & 3 {
            0 => ZigbeeVersion::V0 ,
            1 => ZigbeeVersion::V1 ,
            2 => ZigbeeVersion::V2 ,
            _ => ZigbeeVersion::Invalid ,
        }
    }
}

#[derive(Debug, Clone)]
/// Zigbee Mac Header
pub struct ZigbeeHdr {
    /// Frame type
    pub hdr_type: ZigbeeFrameType,
    /// Frame Version
    pub version: ZigbeeVersion,
    /// Security Enabled
    pub security: bool,
    /// Frame pending
    pub pending : bool,
    /// Acknoledge Requested
    pub ack_req: bool,
    /// IE fields present
    pub has_ie: bool,
    /// Source address
    pub src: Addr,
    /// Destination address
    pub dst: Addr,
    /// Sequence Number
    pub seq_num: Option<u8>
}

// Packet format:
// 2B = Frame Control
// 0/1 = Sequence number
// 0/2 = Dst PAN ID
// 0/2/8 = Dst Address
// 0/2 = Src PAN ID
// 0/2/8 = Src Address
// Variable: Auxiliary security header
// Variable: IE
// Payload

// [41, 88, 0f, e7, 97, ff, ff, 02, 00, 09, 12, fc, ff, 02, 00, 01, 00, 91, 70, e8, 09, 01, 88, 17, 00, 28, 13, 20, 8e, 00, 91, 70, e8, 09, 01, 88, 17, 00, 00, 01, fe, 18, 86, ae, ca]

impl ZigbeeHdr {
    /// Extract Phy Header information from a byte stream
    pub fn parse(iter: &mut impl Iterator<Item = u8>) -> Option<Self> {
        // let mut iter = bytes.iter().copied();
        // Extract first 2 bytes for FrameControl
        let b0 = iter.next()?;
        let b1 = iter.next()?;

        let hdr_type : ZigbeeFrameType = b0.into();
        let version  : ZigbeeVersion = (b1>>4).into();
        //
        let security = (b0&0x08) != 0;
        let pending  = (b0&0x20) != 0;
        let ack_req  = (b0&0x20) != 0;
        let pan_zip  = (b0&0x40) != 0;
        let has_ie   = (b1&0x02) != 0;
        let no_seq   = (b1&0x01) != 0;
        //
        let seq_num = if no_seq {None} else {Some(iter.next()?)};
        let dst_mode = AddrMode::from_byte(b1>>2)?;
        let src_mode = AddrMode::from_byte(b1>>6)?;
        // Addresses
        // Note: condition for presence or not of PAN ID is more complex, this is just good enough for testing
        let dst = Addr::from_bytes(dst_mode, true, iter)?;
        let src = Addr::from_bytes(src_mode, !pan_zip, iter)?;
        Some(Self {
            hdr_type, version,
            security, pending, ack_req, has_ie,
            src,
            dst,
            seq_num
        })
    }
}

impl Format for ZigbeeHdr {
    fn format(&self, fmt: Formatter) {
        write!(fmt, "[{}] sec={}, ie={} ", self.hdr_type, self.security, self.has_ie);
        if let Some(sn) = &self.seq_num {
            write!(fmt, " sn={}", sn);
        }
        if self.ack_req {
            write!(fmt, " (AckReq)");
        }
        write!(fmt, "| {} -> {} | ", self.src, self.dst);
    }
}

#[derive(Debug, Clone, Copy, Format, PartialEq)]
pub enum ZigbeeCmd {
    JoinReq         = 0x01,
    JoinRsp         = 0x02,
    Leaving         = 0x03,
    DataReq         = 0x04,
    PanIdConflict   = 0x05,
    Orphan          = 0x06,
    BeaconReq       = 0x07,
    CoordConfig     = 0x08,
    GtsReq          = 0x09,
    TrleMngmtReq    = 0x0a,
    TrleMngmtRsp    = 0x0b,
    DsmeJoinReq     = 0x13,
    DsmeJoinRsp     = 0x14,
    DsmeGtsReq      = 0x15,
    DsmeGtsRsp      = 0x16,
    DsmeGtsNotify   = 0x17,
    DsmeInfoReq     = 0x18,
    DsmeInfoRsp     = 0x19,
    DsmeBeaconAlloc = 0x1a,
    DsmeBeaconClash = 0x1b,
    DsmeLinkReport  = 0x1c,
    RitDataReq      = 0x20,
    DbsReq          = 0x21,
    DbsRsp          = 0x22,
    RitDataRsp      = 0x23,
    VendorSpecific  = 0x24,
    SrmReq          = 0x25,
    SrmRsp          = 0x26,
    SrmReport       = 0x27,
    SrmInfo         = 0x28,
    Unknown         = 0xFF,
}

impl From<u8> for ZigbeeCmd {
    fn from(value: u8) -> Self {
        match value {
            0x01 => ZigbeeCmd::JoinReq,
            0x02 => ZigbeeCmd::JoinRsp,
            0x03 => ZigbeeCmd::Leaving,
            0x04 => ZigbeeCmd::DataReq,
            0x05 => ZigbeeCmd::PanIdConflict,
            0x06 => ZigbeeCmd::Orphan,
            0x07 => ZigbeeCmd::BeaconReq,
            0x08 => ZigbeeCmd::CoordConfig,
            0x09 => ZigbeeCmd::GtsReq,
            0x0a => ZigbeeCmd::TrleMngmtReq,
            0x0b => ZigbeeCmd::TrleMngmtRsp,
            0x13 => ZigbeeCmd::DsmeJoinReq,
            0x14 => ZigbeeCmd::DsmeJoinRsp,
            0x15 => ZigbeeCmd::DsmeGtsReq,
            0x16 => ZigbeeCmd::DsmeGtsRsp,
            0x17 => ZigbeeCmd::DsmeGtsNotify,
            0x18 => ZigbeeCmd::DsmeInfoReq,
            0x19 => ZigbeeCmd::DsmeInfoRsp,
            0x1a => ZigbeeCmd::DsmeBeaconAlloc,
            0x1b => ZigbeeCmd::DsmeBeaconClash,
            0x1c => ZigbeeCmd::DsmeLinkReport,
            0x20 => ZigbeeCmd::RitDataReq,
            0x21 => ZigbeeCmd::DbsReq,
            0x22 => ZigbeeCmd::DbsRsp,
            0x23 => ZigbeeCmd::RitDataRsp,
            0x24 => ZigbeeCmd::VendorSpecific,
            0x25 => ZigbeeCmd::SrmReq,
            0x26 => ZigbeeCmd::SrmRsp,
            0x27 => ZigbeeCmd::SrmReport,
            0x28 => ZigbeeCmd::SrmInfo,
            _ => ZigbeeCmd::Unknown,
        }
    }
}

// struct ZigbeePacket;