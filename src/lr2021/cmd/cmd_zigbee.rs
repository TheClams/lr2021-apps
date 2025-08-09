// Zigbee commands API

use crate::lr2021::status::Status;
use super::RxBw;

/// The modulation and data rate to be used for RX and TX
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZigbeeMode {
    Oqpsk250 = 0,
    Oqpsk100 = 1,
    Oqpsk25 = 2,
    Bpsk20 = 3,
    Bpsk40 = 4,
}

/// Address filtering enable/disable. Addresses are set using the SetZigbeeAddress command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressOn {
    Disable = 0,
    Enable = 1,
}

/// Set the Rx/Tx mode for FCS (16 bits)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FcsMode {
    FcsOn = 0,
    FcsInFifo = 1,
}

/// Sets the parameters for Zigbee packets
pub fn set_zigbee_params_cmd(zigbee_mode: ZigbeeMode, rx_bw: RxBw, pld_len: u8, pbl_len_tx: u16, address_on: AddressOn, fcs_mode: FcsMode) -> [u8; 9] {
    let mut cmd = [0u8; 9];
    cmd[0] = 0x02;
    cmd[1] = 0x9F;

    cmd[2] |= zigbee_mode as u8;
    cmd[3] |= rx_bw as u8;
    cmd[4] |= pld_len;
    cmd[5] |= (pbl_len_tx & 0xFF) as u8;
    cmd[6] |= ((pbl_len_tx >> 8) & 0xFF) as u8;
    cmd[7] |= ((address_on as u8) & 0x1) << 2;
    cmd[7] |= (fcs_mode as u8) & 0x1;
    cmd
}

/// Gets the status of the last received packet. Status is updated at the end of a reception (RxDone irq), but rssi_sync is already updated on SyncWordValid irq
pub fn get_zigbee_packet_status_req() -> [u8; 2] {
    [0x02, 0xA1]
}

/// Sets the zigbee packet length without the need to call the full SetZigbeeParams command (slower to run)
pub fn set_zigbee_packet_len_cmd(pld_len: u8) -> [u8; 3] {
    let mut cmd = [0u8; 3];
    cmd[0] = 0x02;
    cmd[1] = 0xA2;

    cmd[2] |= pld_len;
    cmd
}

/// Sets the Zigbee addresses for filtering in Rx. Frames that don't match the addresses raise an address error IRQ. When a packet is received, the destination address and PAN ID are both checked. Broadcast addresses are matched as well. Multi-cast is not supported or filtered
pub fn set_zigbee_address_cmd(long_dest_addr: u64, short_dest_addr: u16, pan_id: u16, trans_id: u8) -> [u8; 15] {
    let mut cmd = [0u8; 15];
    cmd[0] = 0x02;
    cmd[1] = 0xA3;

    cmd[2] |= (long_dest_addr & 0xFF) as u8;
    cmd[3] |= ((long_dest_addr >> 8) & 0xFF) as u8;
    cmd[4] |= ((long_dest_addr >> 16) & 0xFF) as u8;
    cmd[5] |= ((long_dest_addr >> 24) & 0xFF) as u8;
    cmd[6] |= ((long_dest_addr >> 32) & 0xFF) as u8;
    cmd[7] |= ((long_dest_addr >> 40) & 0xFF) as u8;
    cmd[8] |= ((long_dest_addr >> 48) & 0xFF) as u8;
    cmd[9] |= ((long_dest_addr >> 56) & 0xFF) as u8;
    cmd[10] |= (short_dest_addr & 0xFF) as u8;
    cmd[11] |= ((short_dest_addr >> 8) & 0xFF) as u8;
    cmd[12] |= (pan_id & 0xFF) as u8;
    cmd[13] |= ((pan_id >> 8) & 0xFF) as u8;
    cmd[14] |= trans_id;
    cmd
}

// Response structs

/// Response for GetZigbeePacketStatus command
#[derive(Default)]
pub struct GetZigbeePacketStatusRsp([u8; 9]);

impl GetZigbeePacketStatusRsp {
    /// Create a new response buffer
    pub fn new() -> Self {
        Self::default()
    }

    /// Return Status
    pub fn status(&mut self) -> Status {
        Status::from_slice(&self.0[..2])
    }

    /// Content of the received PHY header (the frame length)
    pub fn rx_header(&self) -> u8 {
        self.0[2]
    }

    /// Length of the last received packet in bytes (including optional data added in the FIFO, crc, ...)
    pub fn pkt_len(&self) -> u16 {
        (self.0[4] as u16) |
        ((self.0[3] as u16) << 8)
    }

    /// RSSI average over the last packet received. Actual signal power is –rssi_pkt/2 (dBm)
    pub fn rssi_avg(&self) -> u16 {
        (((self.0[7] >> 2) & 0x1) as u16) |
        ((self.0[5] as u16) << 1)
    }

    /// Latched RSSI value after syncword detection. Actual signal power is –rssi_sync/2 (dBm)
    pub fn rssi_sync(&self) -> u16 {
        ((self.0[7] & 0x1) as u16) |
        ((self.0[6] as u16) << 1)
    }

    /// Link quality indicator (0.25dB)
    pub fn lqi(&self) -> u8 {
        self.0[8]
    }
}

impl AsMut<[u8]> for GetZigbeePacketStatusRsp {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}
