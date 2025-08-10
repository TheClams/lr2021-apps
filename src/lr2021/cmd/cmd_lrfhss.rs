// Lrfhss commands API

/// Sets the LR-FHSS syncword. Reset value is { 0x2C, 0x0F, 0x79, 0x95 }
pub fn set_lr_fhss_sync_word_cmd(syncword: u32) -> [u8; 6] {
    let mut cmd = [0u8; 6];
    cmd[0] = 0x02;
    cmd[1] = 0x57;

    cmd[2] |= ((syncword >> 24) & 0xFF) as u8;
    cmd[3] |= ((syncword >> 16) & 0xFF) as u8;
    cmd[4] |= ((syncword >> 8) & 0xFF) as u8;
    cmd[5] |= (syncword & 0xFF) as u8;
    cmd
}

// Commands with variable length parameters (not implemented):
// - LrFhssBuildFrame
