/// Parsing functions for firmware text and binary responses.
/// Separated for testability.

/// Keyboard physical dimensions (must match firmware).
pub const ROWS: usize = 5;
pub const COLS: usize = 13;

/// Parse "TD0: 04,05,06,29" lines into an array of 8 tap dance slots.
/// Each slot has 4 actions: [1-tap, 2-tap, 3-tap, hold].
pub fn parse_td_lines(lines: &[String]) -> Vec<[u16; 4]> {
    let mut result = vec![[0u16; 4]; 8];

    for line in lines {
        // Only process lines starting with "TD"
        let starts_with_td = line.starts_with("TD");
        if !starts_with_td {
            continue;
        }

        // Find the colon separator: "TD0: ..."
        let colon = match line.find(':') {
            Some(i) => i,
            None => continue,
        };

        // Extract the index between "TD" and ":"
        let index_str = &line[2..colon];
        let idx: usize = match index_str.parse() {
            Ok(i) if i < 8 => i,
            _ => continue,
        };

        // Parse the comma-separated hex values after the colon
        let after_colon = &line[colon + 1..];
        let trimmed_values = after_colon.trim();
        let split_parts = trimmed_values.split(',');
        let vals: Vec<u16> = split_parts
            .filter_map(|s| {
                let trimmed_part = s.trim();
                u16::from_str_radix(trimmed_part, 16).ok()
            })
            .collect();

        // We need exactly 4 values
        let has_four_values = vals.len() == 4;
        if has_four_values {
            result[idx] = [vals[0], vals[1], vals[2], vals[3]];
        }
    }

    result
}

/// Parse KO (Key Override) lines into arrays of [trigger, mod, result, res_mod].
/// Format: "KO0: trigger=2A mod=02 -> result=4C resmod=00"
pub fn parse_ko_lines(lines: &[String]) -> Vec<[u8; 4]> {
    let mut result = Vec::new();

    for line in lines {
        // Only process lines starting with "KO"
        let starts_with_ko = line.starts_with("KO");
        if !starts_with_ko {
            continue;
        }

        // Helper: extract hex value after a keyword like "trigger="
        let parse_hex = |key: &str| -> u8 {
            let key_position = line.find(key);

            let after_key = match key_position {
                Some(i) => {
                    let rest = &line[i + key.len()..];
                    let first_token = rest.split_whitespace().next();
                    first_token
                }
                None => None,
            };

            let parsed_value = match after_key {
                Some(s) => {
                    let without_prefix = s.trim_start_matches("0x");
                    u8::from_str_radix(without_prefix, 16).ok()
                }
                None => None,
            };

            parsed_value.unwrap_or(0)
        };

        let trigger = parse_hex("trigger=");
        let modifier = parse_hex("mod=");
        let result_key = parse_hex("result=");
        let result_mod = parse_hex("resmod=");

        result.push([trigger, modifier, result_key, result_mod]);
    }

    result
}

/// Parse heatmap lines (KEYSTATS? response).
/// Format: "R0:   100    50    30    20    10     5     0    15    25    35    45    55    65"
/// Returns (data[5][13], max_value).
pub fn parse_heatmap_lines(lines: &[String]) -> (Vec<Vec<u32>>, u32) {
    let mut data: Vec<Vec<u32>> = vec![vec![0u32; COLS]; ROWS];
    let mut max = 0u32;

    for line in lines {
        let trimmed = line.trim();

        // Only process lines starting with "R"
        if !trimmed.starts_with('R') {
            continue;
        }

        // Find the colon
        let colon = match trimmed.find(':') {
            Some(i) => i,
            None => continue,
        };

        // Extract row number between "R" and ":"
        let row_str = &trimmed[1..colon];
        let row: usize = match row_str.parse() {
            Ok(r) if r < ROWS => r,
            _ => continue,
        };

        // Parse space-separated values after the colon
        let values_str = &trimmed[colon + 1..];
        for (col, token) in values_str.split_whitespace().enumerate() {
            if col >= COLS {
                break;
            }
            let count: u32 = token.parse().unwrap_or(0);
            data[row][col] = count;
            if count > max {
                max = count;
            }
        }
    }

    (data, max)
}

/// Parsed combo: [index, row1, col1, row2, col2, result_keycode]
#[derive(Clone)]
pub struct ComboEntry {
    pub index: u8,
    pub r1: u8,
    pub c1: u8,
    pub r2: u8,
    pub c2: u8,
    pub result: u16,
}

/// Parse "COMBO0: r3c3+r3c4=29" lines.
pub fn parse_combo_lines(lines: &[String]) -> Vec<ComboEntry> {
    let mut result = Vec::new();

    for line in lines {
        let starts_with_combo = line.starts_with("COMBO");
        if !starts_with_combo {
            continue;
        }

        // Find colon: "COMBO0: ..."
        let colon = match line.find(':') {
            Some(i) => i,
            None => continue,
        };

        // Index between "COMBO" and ":"
        let index_str = &line[5..colon];
        let index: u8 = match index_str.parse() {
            Ok(i) => i,
            _ => continue,
        };

        // After colon: "r3c3+r3c4=29"
        let rest = line[colon + 1..].trim();

        // Split by "="
        let eq_parts: Vec<&str> = rest.split('=').collect();
        let has_two_parts = eq_parts.len() == 2;
        if !has_two_parts {
            continue;
        }

        // Split left side by "+"
        let key_parts: Vec<&str> = eq_parts[0].split('+').collect();
        let has_two_keys = key_parts.len() == 2;
        if !has_two_keys {
            continue;
        }

        // Parse "r3c4" format
        let pos1 = parse_rc(key_parts[0].trim());
        let pos2 = parse_rc(key_parts[1].trim());

        let (r1, c1) = match pos1 {
            Some(rc) => rc,
            None => continue,
        };
        let (r2, c2) = match pos2 {
            Some(rc) => rc,
            None => continue,
        };

        // Parse result keycode (hex)
        let result_str = eq_parts[1].trim();
        let result_code: u16 = match u16::from_str_radix(result_str, 16) {
            Ok(v) => v,
            _ => continue,
        };

        result.push(ComboEntry {
            index,
            r1,
            c1,
            r2,
            c2,
            result: result_code,
        });
    }

    result
}

/// Parse "r3c4" into (row, col).
fn parse_rc(s: &str) -> Option<(u8, u8)> {
    let lower = s.to_lowercase();

    let r_pos = lower.find('r');
    let c_pos = lower.find('c');

    let r_idx = match r_pos {
        Some(i) => i,
        None => return None,
    };
    let c_idx = match c_pos {
        Some(i) => i,
        None => return None,
    };

    let row_str = &lower[r_idx + 1..c_idx];
    let col_str = &lower[c_idx + 1..];

    let row: u8 = match row_str.parse() {
        Ok(v) => v,
        _ => return None,
    };
    let col: u8 = match col_str.parse() {
        Ok(v) => v,
        _ => return None,
    };

    Some((row, col))
}

/// Parsed leader sequence entry.
#[derive(Clone)]
pub struct LeaderEntry {
    pub index: u8,
    pub sequence: Vec<u8>,  // HID keycodes
    pub result: u8,
    pub result_mod: u8,
}

/// Parse "LEADER0: 04,->29+00" lines.
pub fn parse_leader_lines(lines: &[String]) -> Vec<LeaderEntry> {
    let mut result = Vec::new();

    for line in lines {
        let starts_with_leader = line.starts_with("LEADER");
        if !starts_with_leader {
            continue;
        }

        let colon = match line.find(':') {
            Some(i) => i,
            None => continue,
        };

        let index_str = &line[6..colon];
        let index: u8 = match index_str.parse() {
            Ok(i) => i,
            _ => continue,
        };

        // After colon: "04,->29+00"
        let rest = line[colon + 1..].trim();

        // Split by "->"
        let arrow_parts: Vec<&str> = rest.split("->").collect();
        let has_two_parts = arrow_parts.len() == 2;
        if !has_two_parts {
            continue;
        }

        // Sequence: comma-separated hex keycodes (trailing comma OK)
        let seq_str = arrow_parts[0].trim().trim_end_matches(',');
        let sequence: Vec<u8> = seq_str
            .split(',')
            .filter_map(|s| {
                let trimmed = s.trim();
                u8::from_str_radix(trimmed, 16).ok()
            })
            .collect();

        // Result: "29+00" = keycode + modifier
        let result_parts: Vec<&str> = arrow_parts[1].trim().split('+').collect();
        let has_result = result_parts.len() == 2;
        if !has_result {
            continue;
        }

        let result_key = match u8::from_str_radix(result_parts[0].trim(), 16) {
            Ok(v) => v,
            _ => continue,
        };
        let result_mod = match u8::from_str_radix(result_parts[1].trim(), 16) {
            Ok(v) => v,
            _ => continue,
        };

        result.push(LeaderEntry {
            index,
            sequence,
            result: result_key,
            result_mod,
        });
    }

    result
}

/// A single macro step: keycode + modifier, or delay.
#[derive(Clone)]
pub struct MacroStep {
    pub keycode: u8,
    pub modifier: u8,
}

impl MacroStep {
    /// Returns true if this step is a delay (keycode 0xFF).
    pub fn is_delay(&self) -> bool {
        self.keycode == 0xFF
    }

    /// Delay in milliseconds (modifier * 10).
    pub fn delay_ms(&self) -> u32 {
        self.modifier as u32 * 10
    }
}

/// A parsed macro entry.
#[derive(Clone)]
pub struct MacroEntry {
    pub slot: u8,
    pub name: String,
    pub steps: Vec<MacroStep>,
}

/// Parse MACROS? text response.
/// Lines can be like:
///   "MACRO 0: CopyPaste [06:01,FF:0A,19:01]"
///   "M0: name=CopyPaste steps=06:01,FF:0A,19:01"
///   or just raw text lines
pub fn parse_macro_lines(lines: &[String]) -> Vec<MacroEntry> {
    let mut result = Vec::new();

    for line in lines {
        // Try format: "MACRO 0: name [steps]" or "M0: ..."
        let trimmed = line.trim();

        // Skip empty or header lines
        let is_empty = trimmed.is_empty();
        if is_empty {
            continue;
        }

        // Try to find slot number
        let has_macro_prefix = trimmed.starts_with("MACRO") || trimmed.starts_with("M");
        if !has_macro_prefix {
            continue;
        }

        let colon = match trimmed.find(':') {
            Some(i) => i,
            None => continue,
        };

        // Extract slot number from prefix
        let prefix_end = trimmed[..colon].trim();
        let digits_start = prefix_end
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or(prefix_end.len());
        let slot_str = &prefix_end[digits_start..];
        let slot: u8 = match slot_str.trim().parse() {
            Ok(s) => s,
            _ => continue,
        };

        let after_colon = trimmed[colon + 1..].trim();

        // Try to parse name and steps from brackets: "CopyPaste [06:01,FF:0A,19:01]"
        let bracket_start = after_colon.find('[');
        let bracket_end = after_colon.find(']');

        let (name, steps_str) = match (bracket_start, bracket_end) {
            (Some(bs), Some(be)) => {
                let name_part = after_colon[..bs].trim().to_string();
                let steps_part = &after_colon[bs + 1..be];
                (name_part, steps_part.to_string())
            }
            _ => {
                // Try "name=X steps=Y" format
                let name_eq = after_colon.find("name=");
                let steps_eq = after_colon.find("steps=");
                match (name_eq, steps_eq) {
                    (Some(ni), Some(si)) => {
                        let name_start = ni + 5;
                        let name_end = si;
                        let n = after_colon[name_start..name_end].trim().to_string();
                        let s = after_colon[si + 6..].trim().to_string();
                        (n, s)
                    }
                    _ => {
                        // Just use the whole thing as name, no steps
                        (after_colon.to_string(), String::new())
                    }
                }
            }
        };

        // Parse steps: "06:01,FF:0A,19:01"
        let mut steps = Vec::new();
        let has_steps = !steps_str.is_empty();
        if has_steps {
            let step_parts = steps_str.split(',');
            for part in step_parts {
                let step_trimmed = part.trim();
                let kv: Vec<&str> = step_trimmed.split(':').collect();
                let has_two = kv.len() == 2;
                if !has_two {
                    continue;
                }
                let key_byte = u8::from_str_radix(kv[0].trim(), 16).unwrap_or(0);
                let mod_byte = u8::from_str_radix(kv[1].trim(), 16).unwrap_or(0);
                steps.push(MacroStep {
                    keycode: key_byte,
                    modifier: mod_byte,
                });
            }
        }

        result.push(MacroEntry {
            slot,
            name,
            steps,
        });
    }

    result
}

// ---------------------------------------------------------------------------
// Binary payload parsers (protocol v2)
// ---------------------------------------------------------------------------
// These functions parse the *payload* bytes extracted from a KR response frame.
// They produce the same data types as the text parsers above.

/// Parse TD_LIST (0x51) binary payload.
/// Format: [count:u8] then count entries of [idx:u8][a1:u8][a2:u8][a3:u8][a4:u8].
/// Returns Vec<[u16; 4]> with 8 slots (same shape as parse_td_lines).
pub fn parse_td_binary(payload: &[u8]) -> Vec<[u16; 4]> {
    let mut result = vec![[0u16; 4]; 8];

    // Need at least 1 byte for count
    let has_count = !payload.is_empty();
    if !has_count {
        return result;
    }

    let count = payload[0] as usize;
    let entry_size = 5; // idx(1) + actions(4)
    let mut offset = 1;

    for _ in 0..count {
        // Bounds check: need 5 bytes for this entry
        let remaining = payload.len().saturating_sub(offset);
        let enough_bytes = remaining >= entry_size;
        if !enough_bytes {
            break;
        }

        let idx = payload[offset] as usize;
        let action1 = payload[offset + 1] as u16;
        let action2 = payload[offset + 2] as u16;
        let action3 = payload[offset + 3] as u16;
        let action4 = payload[offset + 4] as u16;

        let valid_index = idx < 8;
        if valid_index {
            result[idx] = [action1, action2, action3, action4];
        }

        offset += entry_size;
    }

    result
}

/// Parse COMBO_LIST (0x61) binary payload.
/// Format: [count:u8] then count entries of [idx:u8][r1:u8][c1:u8][r2:u8][c2:u8][result:u8].
pub fn parse_combo_binary(payload: &[u8]) -> Vec<ComboEntry> {
    let mut result = Vec::new();

    let has_count = !payload.is_empty();
    if !has_count {
        return result;
    }

    let count = payload[0] as usize;
    let entry_size = 6; // idx + r1 + c1 + r2 + c2 + result
    let mut offset = 1;

    for _ in 0..count {
        let remaining = payload.len().saturating_sub(offset);
        let enough_bytes = remaining >= entry_size;
        if !enough_bytes {
            break;
        }

        let index = payload[offset];
        let r1 = payload[offset + 1];
        let c1 = payload[offset + 2];
        let r2 = payload[offset + 3];
        let c2 = payload[offset + 4];
        let result_code = payload[offset + 5] as u16;

        result.push(ComboEntry {
            index,
            r1,
            c1,
            r2,
            c2,
            result: result_code,
        });

        offset += entry_size;
    }

    result
}

/// Parse LEADER_LIST (0x71) binary payload.
/// Format: [count:u8] then per entry: [idx:u8][seq_len:u8][seq: seq_len bytes][result:u8][result_mod:u8].
pub fn parse_leader_binary(payload: &[u8]) -> Vec<LeaderEntry> {
    let mut result = Vec::new();

    let has_count = !payload.is_empty();
    if !has_count {
        return result;
    }

    let count = payload[0] as usize;
    let mut offset = 1;

    for _ in 0..count {
        // Need at least idx(1) + seq_len(1)
        let remaining = payload.len().saturating_sub(offset);
        let enough_for_header = remaining >= 2;
        if !enough_for_header {
            break;
        }

        let index = payload[offset];
        let seq_len = payload[offset + 1] as usize;
        offset += 2;

        // Need seq_len bytes for sequence + 2 bytes for result+result_mod
        let remaining_after_header = payload.len().saturating_sub(offset);
        let enough_for_body = remaining_after_header >= seq_len + 2;
        if !enough_for_body {
            break;
        }

        let sequence = payload[offset..offset + seq_len].to_vec();
        offset += seq_len;

        let result_key = payload[offset];
        let result_mod = payload[offset + 1];
        offset += 2;

        result.push(LeaderEntry {
            index,
            sequence,
            result: result_key,
            result_mod,
        });
    }

    result
}

/// Parse KO_LIST (0x92) binary payload.
/// Format: [count:u8] then count entries of [idx:u8][trigger_key:u8][trigger_mod:u8][result_key:u8][result_mod:u8].
/// Returns Vec<[u8; 4]> = [trigger_key, trigger_mod, result_key, result_mod] (same as parse_ko_lines).
pub fn parse_ko_binary(payload: &[u8]) -> Vec<[u8; 4]> {
    let mut result = Vec::new();

    let has_count = !payload.is_empty();
    if !has_count {
        return result;
    }

    let count = payload[0] as usize;
    let entry_size = 5; // idx + trigger_key + trigger_mod + result_key + result_mod
    let mut offset = 1;

    for _ in 0..count {
        let remaining = payload.len().saturating_sub(offset);
        let enough_bytes = remaining >= entry_size;
        if !enough_bytes {
            break;
        }

        // idx is payload[offset], but we skip it (not stored in output)
        let trigger_key = payload[offset + 1];
        let trigger_mod = payload[offset + 2];
        let result_key = payload[offset + 3];
        let result_mod = payload[offset + 4];

        result.push([trigger_key, trigger_mod, result_key, result_mod]);

        offset += entry_size;
    }

    result
}

/// Parse BT_QUERY (0x80) binary payload.
/// Format: [active_slot:u8][initialized:u8][connected:u8][pairing:u8]
///         then 3 slot entries: [slot_idx:u8][valid:u8][addr:6 bytes][name_len:u8][name: name_len bytes]
/// Returns Vec<String> of text lines compatible with the UI (same shape as legacy text parsing).
pub fn parse_bt_binary(payload: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();

    // Need at least 4 bytes for the global state header
    let enough_for_header = payload.len() >= 4;
    if !enough_for_header {
        return lines;
    }

    let active_slot = payload[0];
    let initialized = payload[1];
    let connected = payload[2];
    let pairing = payload[3];

    let status_line = format!(
        "BT: slot={} init={} conn={} pairing={}",
        active_slot, initialized, connected, pairing
    );
    lines.push(status_line);

    let mut offset = 4;
    let slot_count = 3;

    for _ in 0..slot_count {
        // Each slot: slot_idx(1) + valid(1) + addr(6) + name_len(1) = 9 bytes minimum
        let remaining = payload.len().saturating_sub(offset);
        let enough_for_slot_header = remaining >= 9;
        if !enough_for_slot_header {
            break;
        }

        let slot_idx = payload[offset];
        let valid = payload[offset + 1];
        let addr_bytes = &payload[offset + 2..offset + 8];
        let name_len = payload[offset + 8] as usize;
        offset += 9;

        // Read the name string
        let remaining_for_name = payload.len().saturating_sub(offset);
        let enough_for_name = remaining_for_name >= name_len;
        if !enough_for_name {
            break;
        }

        let name_bytes = &payload[offset..offset + name_len];
        let name = String::from_utf8_lossy(name_bytes).to_string();
        offset += name_len;

        // Format address as "XX:XX:XX:XX:XX:XX"
        let addr_str = format!(
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            addr_bytes[0], addr_bytes[1], addr_bytes[2],
            addr_bytes[3], addr_bytes[4], addr_bytes[5]
        );

        let slot_line = format!(
            "BT slot {}: valid={} addr={} name={}",
            slot_idx, valid, addr_str, name
        );
        lines.push(slot_line);
    }

    lines
}

/// Parse TAMA_QUERY (0xA0) binary payload (22 bytes fixed).
/// Format: [enabled:u8][state:u8][hunger:u16 LE][happiness:u16 LE][energy:u16 LE]
///         [health:u16 LE][level:u16 LE][xp:u16 LE][total_keys:u32 LE][max_kpm:u32 LE]
/// Returns Vec<String> with one summary line.
pub fn parse_tama_binary(payload: &[u8]) -> Vec<String> {
    let expected_size = 22;
    let enough_bytes = payload.len() >= expected_size;
    if !enough_bytes {
        return vec!["TAMA: invalid payload".to_string()];
    }

    let enabled = payload[0];
    let _state = payload[1];

    let hunger = u16::from_le_bytes([payload[2], payload[3]]);
    let happiness = u16::from_le_bytes([payload[4], payload[5]]);
    let energy = u16::from_le_bytes([payload[6], payload[7]]);
    let health = u16::from_le_bytes([payload[8], payload[9]]);
    let level = u16::from_le_bytes([payload[10], payload[11]]);
    let _xp = u16::from_le_bytes([payload[12], payload[13]]);
    let total_keys = u32::from_le_bytes([payload[14], payload[15], payload[16], payload[17]]);
    let _max_kpm = u32::from_le_bytes([payload[18], payload[19], payload[20], payload[21]]);

    let line = format!(
        "TAMA: Lv{} hunger={} happy={} energy={} health={} keys={} enabled={}",
        level, hunger, happiness, energy, health, total_keys, enabled
    );

    vec![line]
}

/// Parse WPM_QUERY (0x93) binary payload (2 bytes fixed).
/// Format: [wpm:u16 LE]
pub fn parse_wpm_binary(payload: &[u8]) -> String {
    let enough_bytes = payload.len() >= 2;
    if !enough_bytes {
        return "WPM: 0".to_string();
    }

    let wpm = u16::from_le_bytes([payload[0], payload[1]]);

    format!("WPM: {}", wpm)
}

/// Parse LIST_MACROS (0x30) binary payload.
/// Format: [count:u8] then per entry:
///   [idx:u8][keycode:u16 LE][name_len:u8][name: name_len bytes]
///   [keys_len:u8][keys: keys_len bytes][step_count:u8][{kc:u8,mod:u8}... step_count*2 bytes]
pub fn parse_macros_binary(payload: &[u8]) -> Vec<MacroEntry> {
    let mut result = Vec::new();

    let has_count = !payload.is_empty();
    if !has_count {
        return result;
    }

    let count = payload[0] as usize;
    let mut offset = 1;

    for _ in 0..count {
        // Need at least: idx(1) + keycode(2) + name_len(1) = 4
        let remaining = payload.len().saturating_sub(offset);
        let enough_for_prefix = remaining >= 4;
        if !enough_for_prefix {
            break;
        }

        let slot = payload[offset];
        // keycode is stored but not used in MacroEntry (it's the trigger keycode)
        let _keycode = u16::from_le_bytes([payload[offset + 1], payload[offset + 2]]);
        let name_len = payload[offset + 3] as usize;
        offset += 4;

        // Read name
        let remaining_for_name = payload.len().saturating_sub(offset);
        let enough_for_name = remaining_for_name >= name_len;
        if !enough_for_name {
            break;
        }

        let name_bytes = &payload[offset..offset + name_len];
        let name = String::from_utf8_lossy(name_bytes).to_string();
        offset += name_len;

        // Read keys_len + keys (raw key bytes, skipped for MacroEntry)
        let remaining_for_keys_len = payload.len().saturating_sub(offset);
        let enough_for_keys_len = remaining_for_keys_len >= 1;
        if !enough_for_keys_len {
            break;
        }

        let keys_len = payload[offset] as usize;
        offset += 1;

        let remaining_for_keys = payload.len().saturating_sub(offset);
        let enough_for_keys = remaining_for_keys >= keys_len;
        if !enough_for_keys {
            break;
        }

        // Skip the raw keys bytes
        offset += keys_len;

        // Read step_count + steps
        let remaining_for_step_count = payload.len().saturating_sub(offset);
        let enough_for_step_count = remaining_for_step_count >= 1;
        if !enough_for_step_count {
            break;
        }

        let step_count = payload[offset] as usize;
        offset += 1;

        let steps_byte_size = step_count * 2;
        let remaining_for_steps = payload.len().saturating_sub(offset);
        let enough_for_steps = remaining_for_steps >= steps_byte_size;
        if !enough_for_steps {
            break;
        }

        let mut steps = Vec::with_capacity(step_count);
        for i in 0..step_count {
            let step_offset = offset + i * 2;
            let kc = payload[step_offset];
            let md = payload[step_offset + 1];
            steps.push(MacroStep {
                keycode: kc,
                modifier: md,
            });
        }
        offset += steps_byte_size;

        result.push(MacroEntry {
            slot,
            name,
            steps,
        });
    }

    result
}

/// Parse KEYSTATS_BIN (0x40) binary payload.
/// Format: [rows:u8][cols:u8][counts: rows*cols * u32 LE]
/// Returns (heatmap_data, max_value) — same shape as parse_heatmap_lines.
pub fn parse_keystats_binary(payload: &[u8]) -> (Vec<Vec<u32>>, u32) {
    // Need at least 2 bytes for rows and cols
    let enough_for_header = payload.len() >= 2;
    if !enough_for_header {
        return (vec![], 0);
    }

    let rows = payload[0] as usize;
    let cols = payload[1] as usize;
    let total_cells = rows * cols;
    let data_byte_size = total_cells * 4; // each count is u32 LE

    let remaining = payload.len().saturating_sub(2);
    let enough_for_data = remaining >= data_byte_size;
    if !enough_for_data {
        return (vec![], 0);
    }

    let mut data: Vec<Vec<u32>> = vec![vec![0u32; cols]; rows];
    let mut max_value = 0u32;
    let mut offset = 2;

    for row in 0..rows {
        for col in 0..cols {
            let count = u32::from_le_bytes([
                payload[offset],
                payload[offset + 1],
                payload[offset + 2],
                payload[offset + 3],
            ]);
            data[row][col] = count;

            let is_new_max = count > max_value;
            if is_new_max {
                max_value = count;
            }

            offset += 4;
        }
    }

    (data, max_value)
}
