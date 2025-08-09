// Wisun commands API

use crate::lr2021::status::Status;

/// WISun mode selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WisunMode {
    Mode1a = 0,
    Mode1b = 1,
    Mode2a = 2,
    Mode2b = 3,
    Mode3 = 4,
    Mode4a = 5,
    Mode4b = 6,
    Mode5 = 7,
}

/// FCS selection for TX
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FcsTx {
    Fcs4Byte = 0,
    Fcs2Byte = 1,
}

/// Whitening enable
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Whitening {
    NoWhitening = 0,
    Whitening = 1,
}

/// CRC calculation mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrcOn {
    CrcProvided = 0,
    CrcAuto = 1,
}

/// Payload type selection for TX
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeSwitchTx {
    NormalPayload = 0,
    ModeSwitchPayload = 1,
}

/// FEC encoding selection for TX packet
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FecTx {
    None = 0,
    Nrnsc = 1,
    Rsc = 2,
    RscIntlvr = 3,
}

/// Configure the wisun mode (1a, 1b, 2a, 2b, 3, 4a, 4b, 5)
pub fn set_wisun_mode_cmd(wisun_mode: WisunMode, rx_bw: u8) -> [u8; 4] {
    let mut cmd = [0u8; 4];
    cmd[0] = 0x02;
    cmd[1] = 0x70;

    cmd[2] |= (wisun_mode as u8) & 0x7;
    cmd[3] |= rx_bw;
    cmd
}

/// Configure the wisun packet parameters
pub fn set_wisun_packet_params_cmd(fcs_tx: FcsTx, whitening: Whitening, crc_on: CrcOn, mode_switch_tx: ModeSwitchTx, fec_tx: FecTx, frame_len_tx: u16, pbl_len_tx: u8) -> [u8; 10] {
    let mut cmd = [0u8; 10];
    cmd[0] = 0x02;
    cmd[1] = 0x71;

    cmd[2] |= ((fcs_tx as u8) & 0x1) << 5;
    cmd[2] |= ((whitening as u8) & 0x1) << 4;
    cmd[2] |= ((crc_on as u8) & 0x1) << 3;
    cmd[2] |= ((mode_switch_tx as u8) & 0x1) << 2;
    cmd[2] |= (fec_tx as u8) & 0x3;
    cmd[3] |= (frame_len_tx & 0xFF) as u8;
    cmd[4] |= ((frame_len_tx >> 8) & 0xFF) as u8;
    cmd[5] |= pbl_len_tx;
    cmd
}

/// Configure the wisun packet parameters
pub fn set_wisun_packet_params_adv_cmd(fcs_tx: FcsTx, whitening: Whitening, crc_on: CrcOn, mode_switch_tx: ModeSwitchTx, fec_tx: FecTx, frame_len_tx: u16, pbl_len_tx: u8, pbl_detect: u8) -> [u8; 11] {
    let mut cmd = [0u8; 11];
    cmd[0] = 0x02;
    cmd[1] = 0x71;

    cmd[2] |= ((fcs_tx as u8) & 0x1) << 5;
    cmd[2] |= ((whitening as u8) & 0x1) << 4;
    cmd[2] |= ((crc_on as u8) & 0x1) << 3;
    cmd[2] |= ((mode_switch_tx as u8) & 0x1) << 2;
    cmd[2] |= (fec_tx as u8) & 0x3;
    cmd[3] |= (frame_len_tx & 0xFF) as u8;
    cmd[4] |= ((frame_len_tx >> 8) & 0xFF) as u8;
    cmd[5] |= pbl_len_tx;
    cmd[6] |= pbl_detect;
    cmd
}

/// Gets the status of the last received packet. Status is updated at the end of a reception (RxDone irq), but syncword_idx and rssi_sync are already updated on SyncWordValid irq
pub fn get_wisun_packet_status_req() -> [u8; 2] {
    [0x02, 0x73]
}

/// Sets length of frame for TX for normal packets, or header value for mode_switch packets
pub fn set_wisun_packet_len_cmd(frame_len_tx: u16) -> [u8; 4] {
    let mut cmd = [0u8; 4];
    cmd[0] = 0x02;
    cmd[1] = 0x74;

    cmd[2] |= (frame_len_tx & 0xFF) as u8;
    cmd[3] |= ((frame_len_tx >> 8) & 0xFF) as u8;
    cmd
}

// Response structs

/// Response for GetWisunPacketStatus command
#[derive(Default)]
pub struct GetWisunPacketStatusRsp([u8; 11]);

impl GetWisunPacketStatusRsp {
    /// Create a new response buffer
    pub fn new() -> Self {
        Self::default()
    }

    /// Return Status
    pub fn status(&mut self) -> Status {
        Status::from_slice(&self.0[..2])
    }

    /// Raw 16bit received header
    pub fn header(&self) -> u16 {
        (self.0[3] as u16) |
        ((self.0[2] as u16) << 8)
    }

    /// Length of the last received packet in bytes (including optional data added in the FIFO, crc, ...)
    pub fn pkt_len(&self) -> u16 {
        (self.0[5] as u16) |
        ((self.0[4] as u16) << 8)
    }

    /// Average over last packet received of RSSI. Actual signal power is –rssi_avg/2 (dBm)
    pub fn rssi_avg(&self) -> u16 {
        (((self.0[9] >> 2) & 0x1) as u16) |
        ((self.0[6] as u16) << 1)
    }

    /// Latch RSSI value after syncword detection. Actual signal power is –rssi_sync/2 (dBm)
    pub fn rssi_sync(&self) -> u16 {
        ((self.0[9] & 0x1) as u16) |
        ((self.0[7] as u16) << 1)
    }

    /// Index of detected syncword (0/1)
    pub fn syncword_idx(&self) -> bool {
        (self.0[8] >> 7) & 0x1 != 0
    }

    /// Link quality indicator (0.25dB)
    pub fn lqi(&self) -> u8 {
        self.0[10]
    }
}

impl AsMut<[u8]> for GetWisunPacketStatusRsp {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}
