use super::status::{Intr, Status};

/// Create the GetStatus command
pub fn get_status_req() -> [u8; 2] {
    [0x01, 0x00]
}

/// Create the GetAndClearIrqStatus command
pub fn get_and_clear_irq_req() -> [u8; 2] {
    [0x01, 0x17]
}

/// Response part of the GetStatus command
#[derive(Default)]
pub struct GetStatusRsp([u8; 6]);

impl GetStatusRsp {

    /// Create a buffer for response
    pub fn new() -> Self {
        Self::default()
    }

    /// Return Status
    pub fn status(&mut self) -> Status {
        Status::from_slice(&self.0[..2])
    }

    /// Return Interrupt
    pub fn intr(&self) -> Intr {
        Intr::from_slice(&self.0[2..6])
    }

}

impl AsMut<[u8]> for GetStatusRsp {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

/// Create the GetVersion command
pub fn get_version_req() -> [u8; 2] {
    [0x01, 0x01]
}

/// Response part of the GetTemp command
#[derive(Default)]
pub struct GetVersionRsp([u8; 4]);

impl GetVersionRsp {

    /// Create a buffer for response
    pub fn new() -> Self {
        Self::default()
    }

    /// Return Status
    pub fn status(&mut self) -> Status {
        Status::from_slice(&self.0[..2])
    }

    /// Return major version
    pub fn major(&self) -> u8 {
        self.0[2]
    }

    /// Return major version
    pub fn minor(&self) -> u8 {
        self.0[3]
    }

}

impl AsMut<[u8]> for GetVersionRsp {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}


/// Create the ClearErrors command
pub fn clear_errors_cmd() -> [u8; 2] {
    [0x01, 0x11]
}

/// Source for temperature sensor
pub enum TempSource {
    Vbe = 0,
    Xosc = 1,
    Ntc = 2,
}

/// Resolution of temperature measurements
pub enum TempResolution {
    Res8b = 0,
    Res9b = 1,
    Res10b = 2,
    Res11b = 3,
    Res12b = 4,
    Res13b = 5,
}

/// Create the GetTemp request
pub fn get_temp_req(source: TempSource, resolution: TempResolution) -> [u8; 3] {
    let param: u8 = ((source as u8) << 4) | (1 << 3) | (resolution as u8);
    [0x01, 0x25, param]
}

/// Response part of the GetTemp command
#[derive(Default)]
pub struct GetTempRsp([u8; 4]);

impl GetTempRsp {

    /// Create a buffer for response
    pub fn new() -> Self {
        Self::default()
    }

    /// Return Status
    pub fn status(&mut self) -> Status {
        Status::from_slice(&self.0[..2])
    }

    /// Temperature in s13.5
    pub fn value(&self) -> i16 {
        let raw = (self.0[2] as u16) << 5 | ((self.0[3] as u16) >> 3);
        raw as i16 - if (self.0[2] & 0x80) != 0 {1<<13} else {0}
    }
}

impl AsMut<[u8]> for GetTempRsp {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

impl defmt::Format for GetTempRsp {
    fn format(&self, fmt: defmt::Formatter) {
        defmt::write!(fmt, "{}.{:02}", self.0[2] as i8, (self.0[3] as u16 * 100) >> 8);
    }
}
