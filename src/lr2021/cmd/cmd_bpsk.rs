// Bpsk commands API


/// Signal shape (same as shape_filter in the SetAdvancedModulationParams command)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PulseShape {
    None = 0,
    Custom = 1,
    Bt0p3 = 4,
    Bt0p5 = 5,
    Bt0p7 = 6,
    Bt1p0 = 7,
    Bt2p0 = 2,
    Rc0p3 = 8,
    Rc0p5 = 9,
    Rc0p7 = 10,
    Rc1p0 = 11,
    Rrc0p3 = 12,
    Rrc0p4 = 3,
    Rrc0p5 = 13,
    Rrc0p7 = 14,
    Rrc1p0 = 15,
}

/// Enable Differential encoding
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffModeEn {
    Disabled = 0,
    Enabled = 1,
}

/// BPSK mode selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BpskMode {
    Raw = 0,
    Sigfox = 1,
}

/// Sigfox message type (only valid in Sigfox PHY mode)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigfoxMsg {
    App = 0,
    Ctrl = 1,
}

/// Sigfox frame emission rank (only valid in Sigfox PHY mode)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigfoxRank {
    First = 0,
    Second = 1,
    Third = 2,
}

/// Sets the modulation parameters for BPSK packets. FW configures respective modem registers
pub fn set_bpsk_modulation_params_cmd(bitrate: u32, pulse_shape: PulseShape, diff_mode_en: DiffModeEn, diff_mode_init: bool, diff_mode_parity: bool) -> [u8; 10] {
    let mut cmd = [0u8; 10];
    cmd[0] = 0x02;
    cmd[1] = 0x50;

    cmd[2] |= ((bitrate >> 24) & 0xFF) as u8;
    cmd[3] |= ((bitrate >> 16) & 0xFF) as u8;
    cmd[4] |= ((bitrate >> 8) & 0xFF) as u8;
    cmd[5] |= (bitrate & 0xFF) as u8;
    cmd[6] |= ((pulse_shape as u8) & 0xF) << 4;
    cmd[6] |= ((diff_mode_en as u8) & 0x1) << 2;
    if diff_mode_init { cmd[6] |= 2; }
    if diff_mode_parity { cmd[6] |= 1; }
    cmd
}

/// Sets the packet parameters for BPSK packets. FW configures respective modem registers
pub fn set_bpsk_packet_params_cmd(pld_len: u8, bpsk_mode: BpskMode, sigfox_msg: SigfoxMsg, sigfox_rank: SigfoxRank) -> [u8; 6] {
    let mut cmd = [0u8; 6];
    cmd[0] = 0x02;
    cmd[1] = 0x51;

    cmd[2] |= pld_len;
    cmd[3] |= ((bpsk_mode as u8) & 0x3) << 4;
    cmd[3] |= ((sigfox_msg as u8) & 0x1) << 1;
    cmd[3] |= ((sigfox_rank as u8) & 0x3) << 6;
    cmd
}
