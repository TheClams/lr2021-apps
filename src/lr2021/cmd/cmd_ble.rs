// Ble commands API

use crate::lr2021::status::Status;
use super::RxBw;

/// BLE PHY mode selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BleMode {
    Le1mb = 0,
    Le2mb = 1,
    LeCoded500k = 2,
    LeCoded125k = 3,
}

/// CRC in FIFO control
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrcInFifo {
    CrcNotAppended = 0,
    CrcAppended = 1,
}

/// BLE channel type selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelType {
    Advertiser = 0,
    Data16bitHeader = 1,
    Data24bitHeader = 2,
}

/// Configure the modulation parameters for BLE packets
pub fn set_ble_modulation_params_cmd(ble_mode: BleMode) -> [u8; 3] {
    let mut cmd = [0u8; 3];
    cmd[0] = 0x02;
    cmd[1] = 0x60;

    cmd[2] |= (ble_mode as u8) & 0x3;
    cmd
}

/// Configure the modulation parameters for BLE packets
pub fn set_ble_modulation_params_adv_cmd(ble_mode: BleMode, rx_bw: RxBw) -> [u8; 4] {
    let mut cmd = [0u8; 4];
    cmd[0] = 0x02;
    cmd[1] = 0x60;

    cmd[2] |= (ble_mode as u8) & 0x3;
    cmd[3] |= rx_bw as u8;
    cmd
}

/// Sets the BLE channel/packet dependent parameters
pub fn set_ble_channel_params_cmd(crc_in_fifo: CrcInFifo, channel_type: ChannelType, whit_init: u8, crc_init: u32, syncword: u32) -> [u8; 12] {
    let mut cmd = [0u8; 12];
    cmd[0] = 0x02;
    cmd[1] = 0x61;

    cmd[2] |= ((crc_in_fifo as u8) & 0x1) << 4;
    cmd[2] |= (channel_type as u8) & 0xF;
    cmd[3] |= whit_init;
    cmd[4] |= ((crc_init >> 16) & 0xFF) as u8;
    cmd[5] |= ((crc_init >> 8) & 0xFF) as u8;
    cmd[6] |= (crc_init & 0xFF) as u8;
    cmd[7] |= ((syncword >> 24) & 0xFF) as u8;
    cmd[8] |= ((syncword >> 16) & 0xFF) as u8;
    cmd[9] |= ((syncword >> 8) & 0xFF) as u8;
    cmd[10] |= (syncword & 0xFF) as u8;
    cmd
}

/// Configure PDU length to transmit and send a BLE packet. This command is a concatenation of SetBlePduLen(pld_len) and SetTx(0)
pub fn set_ble_tx_cmd(pld_len: u8) -> [u8; 3] {
    let mut cmd = [0u8; 3];
    cmd[0] = 0x02;
    cmd[1] = 0x62;

    cmd[2] |= pld_len;
    cmd
}

/// Gets the status of the last received packet. Status is updated at the end of a reception (RxDone irq), but rssi_sync is already updated on SyncWordValid irq
pub fn get_ble_packet_status_req() -> [u8; 2] {
    [0x02, 0x65]
}

/// Sets PDU length for TX
pub fn set_ble_tx_pdu_len_cmd(pdu_len: u8) -> [u8; 3] {
    let mut cmd = [0u8; 3];
    cmd[0] = 0x02;
    cmd[1] = 0x66;

    cmd[2] |= pdu_len;
    cmd
}

// Response structs

/// Response for GetBlePacketStatus command
#[derive(Default)]
pub struct BlePacketStatusRsp([u8; 8]);

impl BlePacketStatusRsp {
    /// Create a new response buffer
    pub fn new() -> Self {
        Self::default()
    }

    /// Return Status
    pub fn status(&mut self) -> Status {
        Status::from_slice(&self.0[..2])
    }

    /// Length of the last received packet in bytes (including optional data added in the FIFO, crc, ...)
    pub fn pkt_len(&self) -> u16 {
        (self.0[3] as u16) |
        ((self.0[2] as u16) << 8)
    }

    /// Average over last packet received of RSSI. Actual signal power is –rssi_avg/2 (dBm)
    pub fn rssi_avg(&self) -> u16 {
        (((self.0[6] >> 2) & 0x1) as u16) |
        ((self.0[4] as u16) << 1)
    }

    /// Latch RSSI value after syncword detection. Actual signal power is –rssi_sync/2 (dBm)
    pub fn rssi_sync(&self) -> u16 {
        ((self.0[6] & 0x1) as u16) |
        ((self.0[5] as u16) << 1)
    }

    /// Link quality indicator (0.25dB)
    pub fn lqi(&self) -> u8 {
        self.0[7]
    }
}

impl AsMut<[u8]> for BlePacketStatusRsp {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}
