//! Stats analysis: hand balance, finger load, row usage, top keys, bigrams.
//! Transforms raw heatmap data into structured analysis for the stats tab.

use super::keycode;
use super::layout::UNIT_PX;

/// Which hand a column belongs to.
#[derive(Clone, Copy, Debug, PartialEq)]
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

/// Libellés de rangées pour un clavier de `rows` rangées.
/// Cinq rangées gardent les noms KaSe historiques ; quatre rangées perdent
/// « Number », qu'une carte 4 rangées n'a pas. Au-delà, on numérote plutôt que
/// d'inventer une ergonomie.
pub fn row_labels(rows: usize) -> Vec<String> {
    const FIVE: [&str; 5] = ["Number", "Upper", "Home", "Lower", "Thumb"];
    match rows {
        5 => FIVE.iter().map(|s| s.to_string()).collect(),
        4 => FIVE[1..].iter().map(|s| s.to_string()).collect(),
        n => (0..n).map(|r| format!("Row {}", r)).collect(),
    }
}

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

/// Écart horizontal minimal séparant deux mains — 1,5 u. En deçà, c'est
/// l'espacement normal entre deux colonnes ; au-delà, c'est le trou du split.
const HAND_GAP_PX: f32 = 1.5 * UNIT_PX;

/// Comment un layout se partage entre les mains.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HandSplit {
    /// Deux grappes séparées par un trou : les touches à gauche de `boundary_x`
    /// sont de la main gauche.
    TwoHands { boundary_x: f32 },
    /// Une seule grappe — toutes les touches sont de cette main.
    OneHand(Hand),
}

/// Déduire la séparation des mains des positions horizontales des touches.
///
/// Ce qui distingue deux mains d'une demi-matrice, c'est le trou entre les
/// grappes, pas le nombre de colonnes : le milieu d'une demi-matrice tombe au
/// milieu d'une main et la couperait en deux. Quand il n'y a qu'une grappe,
/// *laquelle* n'est pas déductible de la géométrie — seul le nom rapporté par
/// l'appareil le dit.
pub fn detect_hand_split(xs: &[f32], name: &str) -> HandSplit {
    let mut sorted: Vec<f32> = xs.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mut widest = 0.0f32;
    let mut boundary = 0.0f32;
    for pair in sorted.windows(2) {
        let gap = pair[1] - pair[0];
        if gap > widest {
            widest = gap;
            boundary = (pair[0] + pair[1]) / 2.0;
        }
    }

    if widest >= HAND_GAP_PX {
        HandSplit::TwoHands { boundary_x: boundary }
    } else if name.to_ascii_lowercase().contains("right") {
        HandSplit::OneHand(Hand::Right)
    } else {
        HandSplit::OneHand(Hand::Left)
    }
}

/// Main d'une touche, selon la séparation détectée.
pub fn hand_for_x(x: f32, split: HandSplit) -> Hand {
    match split {
        HandSplit::TwoHands { boundary_x } => {
            if x < boundary_x { Hand::Left } else { Hand::Right }
        }
        HandSplit::OneHand(h) => h,
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

/// Compute row usage. The row count comes from the heatmap itself, which the
/// firmware sizes from its own matrix (`KEYSTATS_BIN` header).
pub fn row_usage(heatmap: &[Vec<u32>]) -> Vec<RowUsage> {
    let labels = row_labels(heatmap.len());
    let row_counts: Vec<u32> = heatmap.iter().map(|row| row.iter().sum()).collect();
    let total: u32 = row_counts.iter().sum();

    let mut result = Vec::with_capacity(row_counts.len());
    for (r, &count) in row_counts.iter().enumerate() {
        let pct = if total > 0 { count as f32 / total as f32 * 100.0 } else { 0.0 };
        result.push(RowUsage { name: labels[r].clone(), row: r, count, pct });
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

    all_keys.sort_by_key(|a| std::cmp::Reverse(a.0));
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

#[cfg(test)]
mod hand_split_tests {
    use super::*;

    /// Une main : 7 touches espacées d'1 u, aucun trou.
    fn one_cluster() -> Vec<f32> {
        (0..7).map(|i| i as f32 * UNIT_PX).collect()
    }

    /// Deux mains : deux grappes de 7, séparées par 3 u.
    fn two_clusters() -> Vec<f32> {
        let mut v = one_cluster();
        v.extend((0..7).map(|i| (i as f32 + 9.0) * UNIT_PX));
        v
    }

    #[test]
    fn a_gap_wider_than_the_threshold_means_two_hands() {
        match detect_hand_split(&two_clusters(), "Niphargus") {
            HandSplit::TwoHands { boundary_x } => {
                assert!(boundary_x > 6.0 * UNIT_PX, "frontiere dans le trou: {}", boundary_x);
                assert!(boundary_x < 9.0 * UNIT_PX, "frontiere dans le trou: {}", boundary_x);
            }
            other => panic!("attendu TwoHands, obtenu {:?}", other),
        }
    }

    #[test]
    fn a_single_cluster_is_one_hand_named_by_the_device() {
        assert_eq!(detect_hand_split(&one_cluster(), "Niphargus Left"), HandSplit::OneHand(Hand::Left));
        assert_eq!(detect_hand_split(&one_cluster(), "Niphargus Right"), HandSplit::OneHand(Hand::Right));
    }

    #[test]
    fn a_nameless_single_cluster_defaults_to_the_left_hand() {
        assert_eq!(detect_hand_split(&one_cluster(), ""), HandSplit::OneHand(Hand::Left));
    }

    #[test]
    fn an_empty_layout_is_one_hand_and_does_not_panic() {
        assert_eq!(detect_hand_split(&[], ""), HandSplit::OneHand(Hand::Left));
    }

    #[test]
    fn hand_for_x_reads_the_boundary_and_the_lone_hand() {
        let two = HandSplit::TwoHands { boundary_x: 100.0 };
        assert_eq!(hand_for_x(50.0, two), Hand::Left);
        assert_eq!(hand_for_x(150.0, two), Hand::Right);
        assert_eq!(hand_for_x(50.0, HandSplit::OneHand(Hand::Right)), Hand::Right);
    }
}

#[cfg(test)]
mod row_label_tests {
    use super::*;

    #[test]
    fn five_rows_keep_the_historical_kase_names() {
        assert_eq!(row_labels(5), vec!["Number", "Upper", "Home", "Lower", "Thumb"]);
    }

    #[test]
    fn four_rows_drop_the_number_row_the_board_does_not_have() {
        assert_eq!(row_labels(4), vec!["Upper", "Home", "Lower", "Thumb"]);
    }

    #[test]
    fn an_unknown_row_count_gets_numbered_labels() {
        assert_eq!(row_labels(2), vec!["Row 0", "Row 1"]);
    }

    #[test]
    fn row_usage_follows_the_heatmap_height_not_a_constant() {
        let heatmap = vec![vec![1u32; 7], vec![2u32; 7], vec![3u32; 7], vec![4u32; 7]];
        let usage = row_usage(&heatmap);
        assert_eq!(usage.len(), 4);
        assert_eq!(usage[0].name, "Upper");
        assert_eq!(usage[3].count, 4 * 7);
    }
}
