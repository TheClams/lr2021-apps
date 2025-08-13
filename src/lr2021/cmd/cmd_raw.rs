// Raw commands API


/// TX mode selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxMode {
    Iq = 0,
    Frequency = 1,
    Phase = 2,
}

/// Trigger selection for the Raw IQ capture start
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerStart {
    SoftTrigger = 0,
    Rssi = 1,
    ModemExternal = 2,
    PreambleDetection = 3,
    RxDone = 4,
    SyncFound = 5,
    HicIrq13 = 6,
    Rtc = 7,
}

/// Trigger selection for the Raw IQ capture stop. Automatically stopped on Rx stop
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerStop {
    SoftTrigger = 0,
    Rssi = 1,
    ModemExternal = 2,
    PreambleDetection = 3,
    RxDone = 4,
    SyncFound = 5,
    HicIrq13 = 6,
    Rtc = 7,
}

/// Sets the parameters used for TX. Note: for mode = IQ, tx_sample_num is the number of IQ pairs, so the number of bytes to write into the TX FIFO is 2*tx_sample_num
pub fn set_raw_iq_tx_params_cmd(tx_sample_num: u16, tx_sample_rate: u32, tx_mode: TxMode) -> [u8; 9] {
    let mut cmd = [0u8; 9];
    cmd[0] = 0x02;
    cmd[1] = 0x93;

    cmd[2] |= ((tx_sample_num >> 8) & 0xFF) as u8;
    cmd[3] |= (tx_sample_num & 0xFF) as u8;
    cmd[4] |= ((tx_sample_rate >> 24) & 0xFF) as u8;
    cmd[5] |= ((tx_sample_rate >> 16) & 0xFF) as u8;
    cmd[6] |= ((tx_sample_rate >> 8) & 0xFF) as u8;
    cmd[7] |= (tx_sample_rate & 0xFF) as u8;
    cmd[8] |= (tx_mode as u8) & 0x3;
    cmd
}

/// Sets the raw IQ capture trigger parameters
pub fn set_raw_iq_trigger_cmd(trigger_start: TriggerStart, trigger_stop: TriggerStop) -> [u8; 3] {
    let mut cmd = [0u8; 3];
    cmd[0] = 0x02;
    cmd[1] = 0x94;

    cmd[2] |= ((trigger_start as u8) & 0xF) << 4;
    cmd[2] |= (trigger_stop as u8) & 0xF;
    cmd
}

/// Sets the raw IQ capture trigger parameters
pub fn set_raw_iq_trigger_adv_cmd(trigger_start: TriggerStart, trigger_stop: TriggerStop, rssi_up: u16, rssi_down: u16) -> [u8; 6] {
    let mut cmd = [0u8; 6];
    cmd[0] = 0x02;
    cmd[1] = 0x94;

    cmd[2] |= ((trigger_start as u8) & 0xF) << 4;
    cmd[2] |= (trigger_stop as u8) & 0xF;
    cmd[3] |= ((rssi_up >> 1) & 0xFF) as u8;
    cmd[5] |= (rssi_up & 0xFF) as u8;
    cmd[4] |= ((rssi_down >> 1) & 0xFF) as u8;
    cmd[5] |= ((rssi_down & 0xFF) << 1) as u8;
    cmd
}
