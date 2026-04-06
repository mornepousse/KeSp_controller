/// KaSe Binary CDC Protocol v2
/// Frame: KS(2) + cmd(1) + len(2 LE) + payload(N) + crc8(1)
/// Response: KR(2) + cmd(1) + status(1) + len(2 LE) + payload(N) + crc8(1)

#[allow(dead_code)]
pub mod cmd {
    // System
    pub const VERSION: u8 = 0x01;
    pub const FEATURES: u8 = 0x02;
    pub const DFU: u8 = 0x03;
    pub const PING: u8 = 0x04;

    // Keymap
    pub const SETLAYER: u8 = 0x10;
    pub const SETKEY: u8 = 0x11;
    pub const KEYMAP_CURRENT: u8 = 0x12;
    pub const KEYMAP_GET: u8 = 0x13;
    pub const LAYER_INDEX: u8 = 0x14;
    pub const LAYER_NAME: u8 = 0x15;

    // Layout
    pub const SET_LAYOUT_NAME: u8 = 0x20;
    pub const LIST_LAYOUTS: u8 = 0x21;
    pub const GET_LAYOUT_JSON: u8 = 0x22;

    // Macros
    pub const LIST_MACROS: u8 = 0x30;
    pub const MACRO_ADD: u8 = 0x31;
    pub const MACRO_ADD_SEQ: u8 = 0x32;
    pub const MACRO_DELETE: u8 = 0x33;

    // Statistics
    pub const KEYSTATS_BIN: u8 = 0x40;
    pub const KEYSTATS_RESET: u8 = 0x42;
    pub const BIGRAMS_BIN: u8 = 0x43;
    pub const BIGRAMS_RESET: u8 = 0x45;

    // Tap Dance
    pub const TD_SET: u8 = 0x50;
    pub const TD_LIST: u8 = 0x51;
    pub const TD_DELETE: u8 = 0x52;

    // Combos
    pub const COMBO_SET: u8 = 0x60;
    pub const COMBO_LIST: u8 = 0x61;
    pub const COMBO_DELETE: u8 = 0x62;

    // Leader
    pub const LEADER_SET: u8 = 0x70;
    pub const LEADER_LIST: u8 = 0x71;
    pub const LEADER_DELETE: u8 = 0x72;

    // Bluetooth
    pub const BT_QUERY: u8 = 0x80;
    pub const BT_SWITCH: u8 = 0x81;
    pub const BT_PAIR: u8 = 0x82;
    pub const BT_DISCONNECT: u8 = 0x83;
    pub const BT_NEXT: u8 = 0x84;
    pub const BT_PREV: u8 = 0x85;

    // Features
    pub const AUTOSHIFT_TOGGLE: u8 = 0x90;
    pub const KO_SET: u8 = 0x91;
    pub const KO_LIST: u8 = 0x92;
    pub const KO_DELETE: u8 = 0x93;
    pub const WPM_QUERY: u8 = 0x94;
    pub const TRILAYER_SET: u8 = 0x94;

    // Tamagotchi
    pub const TAMA_QUERY: u8 = 0xA0;
    pub const TAMA_ENABLE: u8 = 0xA1;
    pub const TAMA_DISABLE: u8 = 0xA2;
    pub const TAMA_FEED: u8 = 0xA3;
    pub const TAMA_PLAY: u8 = 0xA4;
    pub const TAMA_SLEEP: u8 = 0xA5;
    pub const TAMA_MEDICINE: u8 = 0xA6;
    pub const TAMA_SAVE: u8 = 0xA7;

    // OTA
    pub const OTA_START: u8 = 0xF0;
    pub const OTA_DATA: u8 = 0xF1;
    pub const OTA_ABORT: u8 = 0xF2;
}

#[allow(dead_code)]
pub mod status {
    pub const OK: u8 = 0x00;
    pub const ERR_UNKNOWN: u8 = 0x01;
    pub const ERR_CRC: u8 = 0x02;
    pub const ERR_INVALID: u8 = 0x03;
    pub const ERR_RANGE: u8 = 0x04;
    pub const ERR_BUSY: u8 = 0x05;
    pub const ERR_OVERFLOW: u8 = 0x06;
}

/// CRC-8/MAXIM (polynomial 0x31, init 0x00)
pub fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0x00;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x31
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// Build a KS request frame.
pub fn ks_frame(cmd_id: u8, payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u16;
    let mut frame = Vec::with_capacity(6 + payload.len());
    frame.push(0x4B); // 'K'
    frame.push(0x53); // 'S'
    frame.push(cmd_id);
    frame.push((len & 0xFF) as u8);
    frame.push((len >> 8) as u8);
    frame.extend_from_slice(payload);
    frame.push(crc8(payload));
    frame
}

/// Build MACRO_ADD_SEQ payload: [slot][name_len][name...][step_count][{kc,mod}...]
pub fn macro_add_seq_payload(slot: u8, name: &str, steps_hex: &str) -> Vec<u8> {
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len().min(255) as u8;

    // Parse hex steps "06:01,FF:0A,19:01" into (kc, mod) pairs
    let mut step_pairs: Vec<(u8, u8)> = Vec::new();
    if !steps_hex.is_empty() {
        for part in steps_hex.split(',') {
            let trimmed = part.trim();
            let kv: Vec<&str> = trimmed.split(':').collect();
            let has_two = kv.len() == 2;
            if !has_two {
                continue;
            }
            let kc = u8::from_str_radix(kv[0].trim(), 16).unwrap_or(0);
            let md = u8::from_str_radix(kv[1].trim(), 16).unwrap_or(0);
            step_pairs.push((kc, md));
        }
    }
    let step_count = step_pairs.len().min(255) as u8;

    let mut payload = Vec::new();
    payload.push(slot);
    payload.push(name_len);
    payload.extend_from_slice(&name_bytes[..name_len as usize]);
    payload.push(step_count);
    for (kc, md) in &step_pairs {
        payload.push(*kc);
        payload.push(*md);
    }
    payload
}

/// Build MACRO_DELETE payload: [slot]
pub fn macro_delete_payload(slot: u8) -> Vec<u8> {
    vec![slot]
}

/// Build COMBO_SET payload: [index][r1][c1][r2][c2][result]
pub fn combo_set_payload(index: u8, r1: u8, c1: u8, r2: u8, c2: u8, result: u8) -> Vec<u8> {
    vec![index, r1, c1, r2, c2, result]
}

/// Build TD_SET payload: [index][a1][a2][a3][a4]
pub fn td_set_payload(index: u8, actions: &[u16; 4]) -> Vec<u8> {
    vec![index, actions[0] as u8, actions[1] as u8, actions[2] as u8, actions[3] as u8]
}

/// Build KO_SET payload: [index][trigger_key][trigger_mod][result_key][result_mod]
pub fn ko_set_payload(index: u8, trig_key: u8, trig_mod: u8, res_key: u8, res_mod: u8) -> Vec<u8> {
    vec![index, trig_key, trig_mod, res_key, res_mod]
}

/// Build LEADER_SET payload: [index][seq_len][seq...][result][result_mod]
pub fn leader_set_payload(index: u8, sequence: &[u8], result: u8, result_mod: u8) -> Vec<u8> {
    let seq_len = sequence.len().min(4) as u8;
    let mut payload = Vec::with_capacity(4 + sequence.len());
    payload.push(index);
    payload.push(seq_len);
    payload.extend_from_slice(&sequence[..seq_len as usize]);
    payload.push(result);
    payload.push(result_mod);
    payload
}

/// Parsed KR response.
#[derive(Debug)]
pub struct KrResponse {
    #[allow(dead_code)]
    pub cmd: u8,
    pub status: u8,
    pub payload: Vec<u8>,
}

impl KrResponse {
    pub fn is_ok(&self) -> bool {
        self.status == status::OK
    }

    pub fn status_name(&self) -> &str {
        match self.status {
            status::OK => "OK",
            status::ERR_UNKNOWN => "ERR_UNKNOWN",
            status::ERR_CRC => "ERR_CRC",
            status::ERR_INVALID => "ERR_INVALID",
            status::ERR_RANGE => "ERR_RANGE",
            status::ERR_BUSY => "ERR_BUSY",
            status::ERR_OVERFLOW => "ERR_OVERFLOW",
            _ => "UNKNOWN",
        }
    }
}

/// Parse a KR response from raw bytes. Returns (response, bytes_consumed).
pub fn parse_kr(data: &[u8]) -> Result<(KrResponse, usize), String> {
    // Find KR magic
    let pos = data
        .windows(2)
        .position(|w| w[0] == 0x4B && w[1] == 0x52)
        .ok_or("No KR header found")?;

    if data.len() < pos + 7 {
        return Err("Response too short".into());
    }

    let cmd = data[pos + 2];
    let status = data[pos + 3];
    let plen = data[pos + 4] as u16 | ((data[pos + 5] as u16) << 8);
    let payload_start = pos + 6;
    let payload_end = payload_start + plen as usize;

    if data.len() < payload_end + 1 {
        return Err(format!(
            "Incomplete response: need {} bytes, got {}",
            payload_end + 1,
            data.len()
        ));
    }

    let payload = data[payload_start..payload_end].to_vec();
    let expected_crc = data[payload_end];
    let actual_crc = crc8(&payload);

    if expected_crc != actual_crc {
        return Err(format!(
            "CRC mismatch: expected 0x{:02X}, got 0x{:02X}",
            expected_crc, actual_crc
        ));
    }

    let consumed = payload_end + 1 - pos;
    Ok((KrResponse { cmd, status, payload }, consumed))
}
