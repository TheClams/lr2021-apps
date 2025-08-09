// Ranging commands API

use crate::lr2021::status::Status;

/// Type of ranging result to return
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    LatestRaw = 0,
    ExtendedRaw = 1,
    GainSteps = 2,
}

/// Extended mode enable: 0=disabled, 1=enabled
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtendedMode {
    Disabled = 0,
    Enabled = 1,
}

/// Spy mode enable: Set to 1 to enable spy mode on the next set_rx. In SPY mode, the radio acts like a normal device in slave mode and will send a response to any request for its address, but will also listen to requests/responses pairs not addressed to it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpyMode {
    Disabled = 0,
    Enabled = 1,
}

/// Sets the ranging Id for this device (used in the slaves)
pub fn set_ranging_addr_cmd(slave_addr: u32, check_length: u8) -> [u8; 7] {
    let mut cmd = [0u8; 7];
    cmd[0] = 0x02;
    cmd[1] = 0x78;

    cmd[2] |= (slave_addr & 0xFF) as u8;
    cmd[3] |= ((slave_addr >> 8) & 0xFF) as u8;
    cmd[4] |= ((slave_addr >> 16) & 0xFF) as u8;
    cmd[5] |= ((slave_addr >> 24) & 0xFF) as u8;
    cmd[6] |= check_length & 0x7;
    cmd
}

/// Sets the ranging Id for the requests (used in the master)
pub fn set_ranging_req_addr_cmd(req_addr: u32) -> [u8; 6] {
    let mut cmd = [0u8; 6];
    cmd[0] = 0x02;
    cmd[1] = 0x79;

    cmd[2] |= (req_addr & 0xFF) as u8;
    cmd[3] |= ((req_addr >> 8) & 0xFF) as u8;
    cmd[4] |= ((req_addr >> 16) & 0xFF) as u8;
    cmd[5] |= ((req_addr >> 24) & 0xFF) as u8;
    cmd
}

/// Gets the ranging result (For Master or spy only). Based on type parameter, different results are returned. The Distance [m] = rng1*150/(2^12*LoraBW), with LoraBW in MHz. For extended mode type=1 results, rng1 and rng2 values should be averaged to get a distance estimation unaffected by Doppler effect
pub fn get_ranging_result_req(kind: Kind) -> [u8; 3] {
    let mut cmd = [0u8; 3];
    cmd[0] = 0x02;
    cmd[1] = 0x7A;

    cmd[2] |= kind as u8;
    cmd
}

/// Sets the Tx->Rx delay for the ranging calibration
pub fn set_ranging_tx_rx_delay_cmd(delay: u32) -> [u8; 6] {
    let mut cmd = [0u8; 6];
    cmd[0] = 0x02;
    cmd[1] = 0x7B;

    cmd[2] |= (delay & 0xFF) as u8;
    cmd[3] |= ((delay >> 8) & 0xFF) as u8;
    cmd[4] |= ((delay >> 16) & 0xFF) as u8;
    cmd[5] |= ((delay >> 24) & 0xFF) as u8;
    cmd
}

/// Sets the ranging specific parameters
pub fn set_ranging_params_cmd(extended_mode: ExtendedMode, spy_mode: SpyMode, nb_symbols: u8) -> [u8; 5] {
    let mut cmd = [0u8; 5];
    cmd[0] = 0x02;
    cmd[1] = 0x7C;

    cmd[2] |= ((extended_mode as u8) & 0x1) << 7;
    cmd[2] |= ((spy_mode as u8) & 0x1) << 6;
    cmd[2] |= nb_symbols & 0x3F;
    cmd
}

/// Gets the ranging counters for ranging exchanges. Statistics are reset on a POR, sleep without memory retention and the command ResetRxStats. Note: for extended ranging mode, the counters are incremented twice, once for each request/response
pub fn get_ranging_stats_req() -> [u8; 2] {
    [0x02, 0x7D]
}

// Response structs

/// Response for GetRangingResult command
#[derive(Default)]
pub struct GetRangingResultRsp([u8; 6]);

impl GetRangingResultRsp {
    /// Create a new response buffer
    pub fn new() -> Self {
        Self::default()
    }

    /// Return Status
    pub fn status(&mut self) -> Status {
        Status::from_slice(&self.0[..2])
    }

    /// First ranging measurement value (always present)
    pub fn rng1(&self) -> u32 {
        (self.0[4] as u32) |
        ((self.0[3] as u32) << 8) |
        ((self.0[2] as u32) << 16)
    }

    /// RSSI value for first ranging measurement (always present)
    pub fn rssi1(&self) -> u8 {
        self.0[5]
    }
}

impl AsMut<[u8]> for GetRangingResultRsp {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

/// Response for GetRangingResult command
#[derive(Default)]
pub struct GetRangingResultRspAdv([u8; 10]);

impl GetRangingResultRspAdv {
    /// Create a new response buffer
    pub fn new() -> Self {
        Self::default()
    }

    /// Return Status
    pub fn status(&mut self) -> Status {
        Status::from_slice(&self.0[..2])
    }

    /// First ranging measurement value (always present)
    pub fn rng1(&self) -> u32 {
        (self.0[4] as u32) |
        ((self.0[3] as u32) << 8) |
        ((self.0[2] as u32) << 16)
    }

    /// RSSI value for first ranging measurement (always present)
    pub fn rssi1(&self) -> u8 {
        self.0[5]
    }

    /// Second ranging measurement value (only for type=1 extended mode)
    pub fn rng2(&self) -> u32 {
        (self.0[8] as u32) |
        ((self.0[7] as u32) << 8) |
        ((self.0[6] as u32) << 16)
    }

    /// RSSI value for second ranging measurement (only for type=1 extended mode)
    pub fn rssi2(&self) -> u8 {
        self.0[9]
    }
}

impl AsMut<[u8]> for GetRangingResultRspAdv {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

/// Response for GetRangingStats command
#[derive(Default)]
pub struct GetRangingStatsRsp([u8; 12]);

impl GetRangingStatsRsp {
    /// Create a new response buffer
    pub fn new() -> Self {
        Self::default()
    }

    /// Return Status
    pub fn status(&mut self) -> Status {
        Status::from_slice(&self.0[..2])
    }

    /// Number of valid ranging exchanges
    pub fn exchange_valid(&self) -> u16 {
        (self.0[3] as u16) |
        ((self.0[2] as u16) << 8)
    }

    /// Number of valid ranging requests
    pub fn request_valid(&self) -> u16 {
        (self.0[5] as u16) |
        ((self.0[4] as u16) << 8)
    }

    /// Number of completed responses
    pub fn response_done(&self) -> u16 {
        (self.0[7] as u16) |
        ((self.0[6] as u16) << 8)
    }

    /// Number of timeouts (For Manager role: no response received from slave; For Subordinate role: no extended request received in extended mode)
    pub fn timeout(&self) -> u16 {
        (self.0[9] as u16) |
        ((self.0[8] as u16) << 8)
    }

    /// Number of discarded requests
    pub fn request_discarded(&self) -> u16 {
        (self.0[11] as u16) |
        ((self.0[10] as u16) << 8)
    }
}

impl AsMut<[u8]> for GetRangingStatsRsp {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}
