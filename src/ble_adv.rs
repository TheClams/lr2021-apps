use defmt::{debug, info, warn, Format, write};

#[derive(Debug, Clone, Copy, Format, PartialEq)]
pub enum BleAdvType {
    /// Scannable and Connectable
    AdvInd = 0,
    /// Directed Connectable
    AdvDirectInd = 1,
    /// Non-Connectable and non-scannable
    AdvNonConnInd = 2,
    /// Scan request
    ScanReq = 3,
    /// Scan response
    ScanRsp = 4,
    /// Connection request
    ConnectInd = 5,
    /// Non-Connectable and scannable
    AdvScanInd = 6,
    /// Extended type
    AdvExtInd = 7,
    /// Invalid Advertising type
    Invalid = 15,
}

impl BleAdvType {
    pub fn is_scan(&self) -> bool {
        matches!(self, BleAdvType::ScanReq | BleAdvType::ScanRsp)
    }

    pub fn is_adv(&self) -> bool {
        matches!(self, BleAdvType::AdvInd | BleAdvType::AdvDirectInd | BleAdvType::AdvNonConnInd | BleAdvType::AdvScanInd | BleAdvType::AdvExtInd)
    }
}

impl From<u8> for BleAdvType {
    fn from(value: u8) -> Self {
        match value&0xF {
            0 => BleAdvType::AdvInd,
            1 => BleAdvType::AdvDirectInd,
            2 => BleAdvType::AdvNonConnInd,
            3 => BleAdvType::ScanReq,
            4 => BleAdvType::ScanRsp,
            5 => BleAdvType::ConnectInd,
            6 => BleAdvType::AdvScanInd,
            7 => BleAdvType::AdvExtInd,
            _ => BleAdvType::Invalid,
        }
    }
}

pub struct BleAdvHeader(pub u8);

impl BleAdvHeader {
    /// Advertising type
    pub fn get_type(&self) -> BleAdvType {
        self.0.into()
    }

    /// True when RX Address is random, false when public
    pub fn rx_addr(&self) -> bool {
        self.0 & 0x80 != 0
    }

    /// True when TX Address is random, false when public
    pub fn tx_addr(&self) -> bool {
        self.0 & 0x40 != 0
    }

    /// Support LE Channel selection Algorithm #2
    pub fn ch_sel(&self) -> bool {
        self.0 & 0x40 != 0
    }
}
#[repr(u8)]
#[derive(Debug, Clone, Copy, Format)]
pub enum BleAdvDataType {
    Flags = 0x01,
    Uuid16bMore = 0x02,
    Uuid16bFull = 0x03,
    Uuid32bMore = 0x04,
    Uuid32bFull = 0x05,
    Uuid128bMore = 0x06,
    Uuid128bFull = 0x07,
    NameShort = 0x08,
    NameFull = 0x09,
    TxPower = 0x0a,
    DeviceId = 0x10,
    ServiceSolicitation = 0x14,
    ServiceData16b = 0x16,
    Appearance = 0x19,
    ServiceData32b = 0x20,
    ServiceData128b = 0x21,
    Uri = 0x24,
    Encrypted = 0x31,
    Manufacturer = 0xff,
    Unknown(u8) = 0,
}


impl From<u8> for BleAdvDataType {
    fn from(value: u8) -> Self {
        match value {
            0x01 => BleAdvDataType::Flags,
            0x02 => BleAdvDataType::Uuid16bMore,
            0x03 => BleAdvDataType::Uuid16bFull,
            0x04 => BleAdvDataType::Uuid32bMore,
            0x05 => BleAdvDataType::Uuid32bFull,
            0x06 => BleAdvDataType::Uuid128bMore,
            0x07 => BleAdvDataType::Uuid128bFull,
            0x08 => BleAdvDataType::NameShort,
            0x09 => BleAdvDataType::NameFull,
            0x0a => BleAdvDataType::TxPower,
            0x10 => BleAdvDataType::DeviceId,
            0x14 => BleAdvDataType::ServiceSolicitation,
            0x16 => BleAdvDataType::ServiceData16b,
            0x19 => BleAdvDataType::Appearance,
            0x20 => BleAdvDataType::ServiceData32b,
            0x21 => BleAdvDataType::ServiceData128b,
            0x24 => BleAdvDataType::Uri,
            0x31 => BleAdvDataType::Encrypted,
            0xff => BleAdvDataType::Manufacturer,
            v => BleAdvDataType::Unknown(v),
        }
    }
}

pub struct BleAdvFlags(pub u8);
impl BleAdvFlags {
    pub fn is_limited(&self) -> bool {
        (self.0&1) != 0
    }
    pub fn is_general(&self) -> bool {
        (self.0&2) != 0
    }
    pub fn is_non_br(&self) -> bool {
        (self.0&4) != 0
    }
    pub fn is_le_br(&self) -> bool {
        (self.0&8) != 0
    }
    pub fn is_prev_used(&self) -> bool {
        (self.0&16) != 0
    }
}

impl Format for BleAdvFlags {
    fn format(&self, fmt: defmt::Formatter) {
        write!(fmt, "Flags : ");
        if self.is_limited()   {write!(fmt, "Limited, ");}
        if self.is_general()   {write!(fmt, "General, ");}
        if self.is_non_br()    {write!(fmt, "Non BR, ");}
        if self.is_le_br()     {write!(fmt, "LE & BR, ");}
        if self.is_prev_used() {write!(fmt, "Prev. used, ");}
    }
}

#[repr(u16)]
#[derive(Debug, Clone, Copy)]
pub enum BleManufacturer {
    Ericsson = 0x0000,
    IBM = 0x0003,
    Microsoft = 0x0006,
    Apple = 0x004C,
    Harman = 0x0057,
    Samsung = 0x0075,
    Creative = 0x0076,
    Garmin = 0x0087,
    STMicroelectronics = 0x0030,
    Nordic = 0x0059,
    GnHearing = 0x0089,
    Sony = 0x012D,
    Imagination = 0x02F9,
    Xiaomi = 0x038F,
    SkullCandy = 0x07C9,
    Unknown(u16) = 0xFFFF,
}


impl From<&[u8]> for BleManufacturer {
    fn from(value: &[u8]) -> Self {
        match value {
            &[0x00,0x00] => BleManufacturer::Ericsson,
            &[0x03,0x00] => BleManufacturer::IBM,
            &[0x06,0x00] => BleManufacturer::Microsoft,
            &[0x4C,0x00] => BleManufacturer::Apple,
            &[0x57,0x00] => BleManufacturer::Harman,
            &[0x59,0x00] => BleManufacturer::Nordic,
            &[0x75,0x00] => BleManufacturer::Samsung,
            &[0x76,0x00] => BleManufacturer::Creative,
            &[0x87,0x00] => BleManufacturer::Garmin,
            &[0x89,0x00] => BleManufacturer::GnHearing,
            &[0x30,0x00] => BleManufacturer::STMicroelectronics,
            &[0x2D,0x01] => BleManufacturer::Sony,
            &[0xF9,0x02] => BleManufacturer::Imagination,
            &[0x8F,0x03] => BleManufacturer::Xiaomi,
            &[0x07,0xC9] => BleManufacturer::SkullCandy,
            &[b0,b1] => BleManufacturer::Unknown(((b1 as u16) << 8) | b0 as u16),
            _ => BleManufacturer::Unknown(0xFFFF)
        }
    }
}

impl Format for BleManufacturer {
    fn format(&self, fmt: defmt::Formatter) {
        match self {
            BleManufacturer::Ericsson => write!(fmt, "Ericsson"),
            BleManufacturer::IBM => write!(fmt, "IBM"),
            BleManufacturer::Microsoft => write!(fmt, "Microsoft"),
            BleManufacturer::Apple => write!(fmt, "Apple"),
            BleManufacturer::Harman => write!(fmt, "Harman"),
            BleManufacturer::Nordic => write!(fmt, "Nordic"),
            BleManufacturer::Samsung => write!(fmt, "Samsung"),
            BleManufacturer::Creative => write!(fmt, "Creative Labs"),
            BleManufacturer::Garmin => write!(fmt, "Garmin"),
            BleManufacturer::GnHearing => write!(fmt, "GnHearing"),
            BleManufacturer::STMicroelectronics => write!(fmt, "STMicroelectronics"),
            BleManufacturer::Sony => write!(fmt, "Sony"),
            BleManufacturer::Imagination => write!(fmt, "Imagination"),
            BleManufacturer::Xiaomi => write!(fmt, "Xiaomi"),
            BleManufacturer::SkullCandy => write!(fmt, "SkullCandy"),
            BleManufacturer::Unknown(id) => write!(fmt, "{:04x}", id)
        }
    }
}

#[repr(u16)]
#[derive(Debug, Clone, Copy)]
pub enum BleUuid16b {
    AudioSource = 0x110A,
    GenericAudio = 0x1203,
    Hid = 0x1812,
    PhilipsLighting = 0xFE0F,
    Google = 0xFEF3,
    Unknown(u16) = 0xFFFF,
}


impl From<&[u8]> for BleUuid16b {
    fn from(value: &[u8]) -> Self {
        match value {
            &[0x0A,0x11] => BleUuid16b::AudioSource,
            &[0x03,0x12] => BleUuid16b::GenericAudio,
            &[0x12,0x18] => BleUuid16b::Hid,
            &[0x0F,0xFE] |
            &[0x4B,0xFE] => BleUuid16b::PhilipsLighting,
            &[0x2C,0xFE] |
            &[0xF3,0xFE] |
            &[0xF4,0xFE] |
            &[0xF1,0xFC] => BleUuid16b::Google,
            &[b0,b1] => BleUuid16b::Unknown(((b1 as u16) << 8) | b0 as u16),
            _ => BleUuid16b::Unknown(0xFFFF)
        }
    }
}

impl Format for BleUuid16b {
    fn format(&self, fmt: defmt::Formatter) {
        match self {
            BleUuid16b::Hid => write!(fmt, "Human Interface Device"),
            BleUuid16b::AudioSource => write!(fmt, "Audio Source"),
            BleUuid16b::GenericAudio => write!(fmt, "Generic Audio"),
            BleUuid16b::PhilipsLighting => write!(fmt, "Philips Lighting"),
            BleUuid16b::Google => write!(fmt, "Google"),
            BleUuid16b::Unknown(id) => write!(fmt, "{:04x}", id)
        }
    }
}

pub fn parse_ble_adv_hdr(bytes: &[u8]) -> Option<(BleAdvHeader, u64)> {
    let hdr = bytes.first().map(|&b| BleAdvHeader(b)).unwrap_or(BleAdvHeader(0xFF));
    let len = bytes.get(1).map(|&b| b as usize).unwrap_or(0);
    let hdr_type = hdr.get_type();
    let bad_format = len + 2 != bytes.len()
        || len < 9
        || hdr_type==BleAdvType::Invalid
        || hdr_type==BleAdvType::ScanReq && len!=12;
    if bad_format {
        // warn!("[BleAdv] Bad Format: {}, len={} vs {} | Payload = {:02x}", hdr_type, len+2, bytes.len(), bytes);
        None
    } else {
        let addr = ((bytes[2] as u64) << 40) | ((bytes[3] as u64) << 32) | ((bytes[4] as u64) << 24)
                | ((bytes[5] as u64) << 16) | ((bytes[6] as u64) << 8) |  bytes[7] as u64 ;
        Some((hdr,addr))
    }

}

pub fn parse_and_print_ble_adv(addr_seen: &mut AddrList, bytes: &[u8], rssi_dbm: u16, verbose: bool) {
    let Some((hdr, addr)) = parse_ble_adv_hdr(bytes) else {
        // show payload if non-advertising message and verbose is enable
        if verbose {
            debug!("Payload = {:02x} | RSSI -{}dBm", bytes, rssi_dbm);
        }
        return;
    };
    print_ble_adv(addr_seen, bytes, hdr, addr, rssi_dbm);
}

pub fn print_ble_adv(addr_seen: &mut AddrList, bytes: &[u8], hdr: BleAdvHeader, addr: u64, rssi_dbm: u16) {
    let hdr_type = hdr.get_type();
    // Skip Advertising packet with address already observed
    if addr_seen.contains(addr) && hdr_type.is_adv() {
        return;
    }
    //
    let txa = if hdr.tx_addr() {'R'} else {'P'};
    let rxa = if hdr.rx_addr() {'R'} else {'P'};
    match hdr_type {
        // Scan request payload is simply an address on 6 bytes
        BleAdvType::ScanReq => {
            // Length already checked, bytes is known to be 14 bytes at this point
            let addr_scan = ((bytes[8] as u64) << 40) | ((bytes[9] as u64) << 32) | ((bytes[10] as u64) << 24)
                    | ((bytes[11] as u64) << 16) | ((bytes[12] as u64) << 8) |  bytes[13] as u64 ;
            info!("[{}] From {:06x} to {:06x} | RSSI -{}dBm", hdr_type, addr, addr_scan, rssi_dbm);
        }
        BleAdvType::ConnectInd => {
            // Length already checked, bytes is known to be 14 bytes at this point
            let addr_conn = ((bytes[8] as u64) << 40) | ((bytes[9] as u64) << 32) | ((bytes[10] as u64) << 24)
                    | ((bytes[11] as u64) << 16) | ((bytes[12] as u64) << 8) |  bytes[13] as u64 ;
            info!("[{}] From {:06x} to {:06x} | LL Data = {=[u8]:02x} | RSSI -{}dBm", hdr_type, addr, addr_conn, bytes[14..], rssi_dbm);
        }
        // Parse Advertising Data blocks
        _ => {
            info!("[{}] TxA={}, RxAdd={} | Addr 0x{:06x} | RSSI -{}dBm",
                hdr_type, txa, rxa, addr, rssi_dbm);
            let idx = 8;
            print_ble_adv_blocks(idx, bytes);
        }
    }
    if !hdr_type.is_scan() {
        addr_seen.push(addr);
    }
}

pub fn print_ble_adv_blocks(mut idx: usize, bytes: &[u8]) {
    while let Some(l) = bytes.get(idx).map(|&l| l as usize) {
        if bytes.len() < idx + l + 1 {
            warn!("  - Field Incomplete: idx={}, l={}, max={} | {:02x} | Full payload = {:02x}",
                idx, l, bytes.len(), bytes[idx..], bytes);
        }
        else if l > 0 {
            let t : BleAdvDataType = bytes.get(idx+1).copied().unwrap_or(0).into();
            match t {
                BleAdvDataType::Flags => info!("  - {}", BleAdvFlags(bytes[idx+2])),
                BleAdvDataType::Uuid16bMore  |
                BleAdvDataType::Uuid16bFull  => {
                    let id : BleUuid16b = bytes[idx+2..idx+4].into();
                    info!("  - {}: {}", t, id);
                }
                // BleAdvDataType::Uuid32bMore  |
                // BleAdvDataType::Uuid32bFull  => todo!(),
                // BleAdvDataType::Uuid128bMore |
                // BleAdvDataType::Uuid128bFull => todo!(),
                BleAdvDataType::ServiceData16b => {
                    let id : BleUuid16b = bytes[idx+2..idx+4].into();
                    if l > 2 {
                        info!("  - {}: {} | {:02x}", t, id, bytes[idx+4..]);
                    } else {
                        info!("  - {}: {}", t, id);
                    }
                }
                // BleAdvDataType::ServiceData32b => todo!(),
                // BleAdvDataType::ServiceData128b => todo!(),
                // BleAdvDataType::Appearance => todo!(),
                // BleAdvDataType::Uri            => todo!(),
                BleAdvDataType::Manufacturer => {
                    let m : BleManufacturer = bytes[idx+2..idx+4].into();
                    info!("  - {}: {} | {:02x}", t, m, bytes[idx+4..]);
                }
                BleAdvDataType::Unknown(v) => warn!("  - Invalid datatype {}", v),
                BleAdvDataType::NameShort |
                BleAdvDataType::NameFull => info!("  - {}: {=[u8]:a}", t, bytes[idx+2..idx+l+1]),
                BleAdvDataType::TxPower => info!("  - {}: {}", t, bytes[idx+2]),
                _ =>  info!("  - {}: {=[u8]:02x}", t, bytes[idx+2..idx+l+1]),
            }

        }
        idx += l + 1;
    }
}


#[derive(Debug)]
/// Small list of address seen
pub struct AddrList {
    /// List of address seen with valid Advertising message
    addr: [u64;32],
    /// Index of the nex address to fill
    idx: usize,
    /// True when all address in the list are valid
    full: bool,
    /// One address to ignore
    ignore: u64,
}

impl AddrList {

    pub fn new(ignore: u64) -> Self {
        Self {
            addr: [0;32],
            idx: 0,
            full: false,
            ignore
        }
    }

    pub fn size(&self) -> usize {
        if self.full {32} else {self.idx}
    }

    pub fn contains(&self, addr: u64) -> bool {
        let nb = self.size();
        self.addr.iter().take(nb).any(|&a| a==addr)
    }

    pub fn push(&mut self, addr: u64)  {
        if !self.contains(addr) && addr!=self.ignore {
            self.addr[self.idx] = addr;
            if self.idx==31 {
                self.full = true;
            }
            self.idx = (self.idx+1) & 31;
        }
    }

    pub fn clear(&mut self) {
        for a in self.addr.iter_mut() {
            *a = 0;
        }
        self.idx = 0;
        self.full = false;
    }

    pub fn iter(&self) -> core::iter::Take<core::slice::Iter<'_, u64>> {
        self.addr.iter().take(self.size())
    }
}


impl Format for AddrList {
    fn format(&self, fmt: defmt::Formatter) {
        for a in self.addr.iter().take(self.size()) {
            write!(fmt, "{:06x} ", a);
        }
    }
}
