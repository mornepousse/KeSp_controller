/// Stats analysis: hand balance, finger load, row usage, top keys, bigrams.
/// Transforms raw heatmap data into structured analysis for the stats tab.

use super::keycode;
use super::parsers::ROWS;

/// Which hand a column belongs to.
#[derive(Clone, Copy, PartialEq)]
pub enum Hand {
    Left,
    Right,
}

/// Which finger a column belongs to.
#[derive(Clone, Copy, PartialEq)]
pub enum Finger {
    Pinky,
    Ring,
    Middle,
    Index,
    Thumb,
}

/// Row names for the 5 rows.
const ROW_NAMES: [&str; 5] = ["Number", "Upper", "Home", "Lower", "Thumb"];

/// Finger names (French).
const FINGER_NAMES: [&str; 5] = ["Pinky", "Ring", "Middle", "Index", "Thumb"];

/// Map column index → (Hand, Finger).
/// KaSe layout: cols 0-5 = left hand, cols 6 (gap), cols 7-12 = right hand.
fn col_to_hand_finger(col: usize) -> (Hand, Finger) {
    match col {
        0 => (Hand::Left, Finger::Pinky),
        1 => (Hand::Left, Finger::Ring),
        2 => (Hand::Left, Finger::Middle),
        3 => (Hand::Left, Finger::Index),
        4 => (Hand::Left, Finger::Index),   // inner column, still index
        5 => (Hand::Left, Finger::Thumb),
        6 => (Hand::Left, Finger::Thumb),   // center / gap
        7 => (Hand::Right, Finger::Thumb),
        8 => (Hand::Right, Finger::Index),
        9 => (Hand::Right, Finger::Index),  // inner column
        10 => (Hand::Right, Finger::Middle),
        11 => (Hand::Right, Finger::Ring),
        12 => (Hand::Right, Finger::Pinky),
        _ => (Hand::Left, Finger::Pinky),
    }
}

/// Hand balance result.
#[allow(dead_code)]
pub struct HandBalance {
    pub left_count: u32,
    pub right_count: u32,
    pub total: u32,
    pub left_pct: f32,
    pub right_pct: f32,
}

/// Finger load for one finger.
#[allow(dead_code)]
pub struct FingerLoad {
    pub name: String,
    pub hand: Hand,
    pub count: u32,
    pub pct: f32,
}

/// Row usage for one row.
#[allow(dead_code)]
pub struct RowUsage {
    pub name: String,
    pub row: usize,
    pub count: u32,
    pub pct: f32,
}

/// A key in the top keys ranking.
#[allow(dead_code)]
pub struct TopKey {
    pub name: String,
    pub finger: String,
    pub count: u32,
    pub pct: f32,
}

/// Compute hand balance from heatmap data.
pub fn hand_balance(heatmap: &[Vec<u32>]) -> HandBalance {
    let mut left: u32 = 0;
    let mut right: u32 = 0;

    for row in heatmap {
        for (c, &count) in row.iter().enumerate() {
            let (hand, _) = col_to_hand_finger(c);
            match hand {
                Hand::Left => left += count,
                Hand::Right => right += count,
            }
        }
    }

    let total = left + right;
    let left_pct = if total > 0 { left as f32 / total as f32 * 100.0 } else { 0.0 };
    let right_pct = if total > 0 { right as f32 / total as f32 * 100.0 } else { 0.0 };

    HandBalance { left_count: left, right_count: right, total, left_pct, right_pct }
}

/// Compute finger load (10 fingers: 5 left + 5 right).
pub fn finger_load(heatmap: &[Vec<u32>]) -> Vec<FingerLoad> {
    let mut counts = [[0u32; 5]; 2]; // [hand][finger]

    for row in heatmap {
        for (c, &count) in row.iter().enumerate() {
            let (hand, finger) = col_to_hand_finger(c);
            let hi = if hand == Hand::Left { 0 } else { 1 };
            let fi = match finger {
                Finger::Pinky => 0,
                Finger::Ring => 1,
                Finger::Middle => 2,
                Finger::Index => 3,
                Finger::Thumb => 4,
            };
            counts[hi][fi] += count;
        }
    }

    let total: u32 = counts[0].iter().sum::<u32>() + counts[1].iter().sum::<u32>();
    let mut result = Vec::with_capacity(10);

    // Left hand fingers
    for fi in 0..5 {
        let count = counts[0][fi];
        let pct = if total > 0 { count as f32 / total as f32 * 100.0 } else { 0.0 };
        let name = format!("{} L", FINGER_NAMES[fi]);
        result.push(FingerLoad { name, hand: Hand::Left, count, pct });
    }

    // Right hand fingers
    for fi in 0..5 {
        let count = counts[1][fi];
        let pct = if total > 0 { count as f32 / total as f32 * 100.0 } else { 0.0 };
        let name = format!("{} R", FINGER_NAMES[fi]);
        result.push(FingerLoad { name, hand: Hand::Right, count, pct });
    }

    result
}

/// Compute row usage.
pub fn row_usage(heatmap: &[Vec<u32>]) -> Vec<RowUsage> {
    let mut row_counts = [0u32; ROWS];

    for (r, row) in heatmap.iter().enumerate() {
        if r >= ROWS { break; }
        let row_sum: u32 = row.iter().sum();
        row_counts[r] = row_sum;
    }

    let total: u32 = row_counts.iter().sum();
    let mut result = Vec::with_capacity(ROWS);

    for r in 0..ROWS {
        let count = row_counts[r];
        let pct = if total > 0 { count as f32 / total as f32 * 100.0 } else { 0.0 };
        let name = ROW_NAMES[r].to_string();
        result.push(RowUsage { name, row: r, count, pct });
    }

    result
}

/// Compute top N keys by press count.
pub fn top_keys(heatmap: &[Vec<u32>], keymap: &[Vec<u16>], n: usize) -> Vec<TopKey> {
    let mut all_keys: Vec<(u32, usize, usize)> = Vec::new(); // (count, row, col)

    for (r, row) in heatmap.iter().enumerate() {
        for (c, &count) in row.iter().enumerate() {
            if count > 0 {
                all_keys.push((count, r, c));
            }
        }
    }

    all_keys.sort_by(|a, b| b.0.cmp(&a.0));
    all_keys.truncate(n);

    let total: u32 = heatmap.iter().flat_map(|r| r.iter()).sum();

    let mut result = Vec::with_capacity(n);
    for (count, r, c) in all_keys {
        let code = keymap.get(r).and_then(|row| row.get(c)).copied().unwrap_or(0);
        let name = keycode::decode_keycode(code);
        let (hand, finger) = col_to_hand_finger(c);
        let hand_str = if hand == Hand::Left { "L" } else { "R" };
        let finger_str = match finger {
            Finger::Pinky => "Pinky",
            Finger::Ring => "Ring",
            Finger::Middle => "Middle",
            Finger::Index => "Index",
            Finger::Thumb => "Thumb",
        };
        let finger_label = format!("{} {}", finger_str, hand_str);
        let pct = if total > 0 { count as f32 / total as f32 * 100.0 } else { 0.0 };
        result.push(TopKey { name, finger: finger_label, count, pct });
    }

    result
}

/// Find keys that have never been pressed (count = 0, keycode != 0).
pub fn dead_keys(heatmap: &[Vec<u32>], keymap: &[Vec<u16>]) -> Vec<String> {
    let mut result = Vec::new();
    for (r, row) in heatmap.iter().enumerate() {
        for (c, &count) in row.iter().enumerate() {
            if count > 0 { continue; }
            let code = keymap.get(r).and_then(|row| row.get(c)).copied().unwrap_or(0);
            let is_mapped = code != 0;
            if is_mapped {
                let name = keycode::decode_keycode(code);
                result.push(name);
            }
        }
    }
    result
}

// ==================== Bigram analysis ====================

/// A parsed bigram entry.
pub struct BigramEntry {
    pub from_row: u8,
    pub from_col: u8,
    pub to_row: u8,
    pub to_col: u8,
    pub count: u32,
}

/// Bigram analysis results.
#[allow(dead_code)]
pub struct BigramAnalysis {
    pub total: u32,
    pub alt_hand: u32,
    pub same_hand: u32,
    pub sfb: u32,
    pub alt_hand_pct: f32,
    pub same_hand_pct: f32,
    pub sfb_pct: f32,
}

/// Parse bigram text lines from firmware.
/// Format: "  R2C3 -> R2C4 : 150"
pub fn parse_bigram_lines(lines: &[String]) -> Vec<BigramEntry> {
    let mut entries = Vec::new();

    for line in lines {
        let trimmed = line.trim();
        let has_arrow = trimmed.contains("->");
        if !has_arrow { continue; }

        let parts: Vec<&str> = trimmed.split("->").collect();
        if parts.len() != 2 { continue; }

        let left = parts[0].trim();
        let right_and_count = parts[1].trim();

        let right_parts: Vec<&str> = right_and_count.split(':').collect();
        if right_parts.len() != 2 { continue; }

        let right = right_parts[0].trim();
        let count_str = right_parts[1].trim();

        let from = parse_rc(left);
        let to = parse_rc(right);
        let count: u32 = count_str.parse().unwrap_or(0);

        if let (Some((fr, fc)), Some((tr, tc))) = (from, to) {
            entries.push(BigramEntry {
                from_row: fr, from_col: fc,
                to_row: tr, to_col: tc,
                count,
            });
        }
    }

    entries
}

/// Parse "R2C3" into (row, col).
fn parse_rc(s: &str) -> Option<(u8, u8)> {
    let s = s.trim();
    let r_pos = s.find('R')?;
    let c_pos = s.find('C')?;
    if c_pos <= r_pos { return None; }

    let row_str = &s[r_pos + 1..c_pos];
    let col_str = &s[c_pos + 1..];
    let row: u8 = row_str.parse().ok()?;
    let col: u8 = col_str.parse().ok()?;
    Some((row, col))
}

/// Analyze bigram entries for hand alternation and SFB.
pub fn analyze_bigrams(entries: &[BigramEntry]) -> BigramAnalysis {
    let mut alt_hand: u32 = 0;
    let mut same_hand: u32 = 0;
    let mut sfb: u32 = 0;
    let mut total: u32 = 0;

    for entry in entries {
        let (hand_from, finger_from) = col_to_hand_finger(entry.from_col as usize);
        let (hand_to, finger_to) = col_to_hand_finger(entry.to_col as usize);

        total += entry.count;

        if hand_from != hand_to {
            alt_hand += entry.count;
        } else {
            same_hand += entry.count;
            if finger_from == finger_to {
                sfb += entry.count;
            }
        }
    }

    let alt_hand_pct = if total > 0 { alt_hand as f32 / total as f32 * 100.0 } else { 0.0 };
    let same_hand_pct = if total > 0 { same_hand as f32 / total as f32 * 100.0 } else { 0.0 };
    let sfb_pct = if total > 0 { sfb as f32 / total as f32 * 100.0 } else { 0.0 };

    BigramAnalysis {
        total, alt_hand, same_hand, sfb,
        alt_hand_pct, same_hand_pct, sfb_pct,
    }
}
