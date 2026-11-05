//! Looks over a whole keymap for what the firmware would not do as hoped. Rules
//! come from the firmware's sources (see MECHANICS.md). Pure: no UI, no serial.

use crate::protocol::keycode;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Level {
    Warning,
    Note,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub level: Level,
    pub text: String,
    /// (layer, row, col) to jump to, when it is about one key.
    pub at: Option<(usize, usize, usize)>,
    /// Layer to jump to otherwise.
    pub layer: Option<usize>,
    /// The key does nothing as it stands: offer to empty it.
    pub clearable: bool,
}

/// One key of the matrix on one layer.
#[derive(Clone, Copy, Debug)]
pub struct Key {
    pub layer: usize,
    pub row: usize,
    pub col: usize,
    pub code: u16,
    pub on_right_half: bool,
}

pub struct Input<'a> {
    pub keys: &'a [Key],
    pub layer_names: &'a [String],
    /// Macro slots that exist. `None` until the keyboard's list is read.
    pub macros: Option<&'a [u8]>,
    /// Tap dance slots with an action. `None` until the list is read.
    pub tap_dances: Option<&'a [usize]>,
}

/// Layer a key switches to, and whether it stays there after release.
fn layer_target(code: u16) -> Option<(usize, bool)> {
    let low_zero = code & 0xFF == 0;
    match code {
        0x0100..=0x0A00 if low_zero => Some(((code >> 8) as usize - 1, false)),
        0x0B00..=0x1400 if low_zero => Some(((code >> 8) as usize - 0x0B, true)),
        0x3100..=0x310F => Some(((code & 0x0F) as usize, false)),
        0x4000..=0x4FFF => Some((((code >> 8) & 0x0F) as usize, false)),
        0x7000..=0x7FFF => Some(((code & 0x0F) as usize, false)),
        _ => None,
    }
}

const MACRO_FIRST: u16 = 0x1500;
const MACRO_LAST: u16 = 0x2800;
const SEC_CONFIRM: u16 = 0x3E00;
const INTERNAL_WPM: u16 = 0x3B00;

fn is_macro(code: u16) -> Option<u8> {
    ((MACRO_FIRST..=MACRO_LAST).contains(&code) && code & 0xFF == 0).then(|| ((code - MACRO_FIRST) >> 8) as u8)
}

fn is_tap_dance(code: u16) -> Option<usize> {
    (0x6000..=0x6FFF).contains(&code).then_some(((code >> 8) & 0x0F) as usize)
}

fn name_of(names: &[String], layer: usize) -> String {
    names.get(layer).cloned().unwrap_or_else(|| format!("Layer {layer}"))
}

pub fn run(input: &Input) -> Vec<Finding> {
    let mut out = Vec::new();
    let names = input.layer_names;
    let has_keys = |l: usize| input.keys.iter().any(|k| k.layer == l && k.code != 0);

    // Which layers can be reached, starting from the base one.
    let mut reached = vec![0usize];
    let mut i = 0;
    while i < reached.len() {
        let from = reached[i];
        i += 1;
        for k in input.keys.iter().filter(|k| k.layer == from) {
            if let Some((to, _)) = layer_target(k.code) {
                if !reached.contains(&to) {
                    reached.push(to);
                }
            }
        }
    }
    for l in 1..names.len() {
        if has_keys(l) && !reached.contains(&l) {
            out.push(Finding {
                level: Level::Warning,
                text: format!("{}: has keys but no key reaches it (unless a Tri-Layer rule does)", name_of(names, l)),
                at: None,
                layer: Some(l),
                clearable: false,
            });
        }
    }

    // A TO stays on after release. Only another TO leaves: an MO returns to the
    // layer it left, and empty keys fall back onto this same layer.
    for k in input.keys.iter().filter(|k| reached.contains(&k.layer)) {
        let Some((to, true)) = layer_target(k.code) else { continue };
        if to == 0 || to == k.layer {
            continue;
        }
        let way_out = input
            .keys
            .iter()
            .any(|o| o.layer == to && matches!(layer_target(o.code), Some((_, true))));
        if !way_out {
            out.push(Finding {
                level: Level::Warning,
                text: format!(
                    "{}: {} enters it for good, and no TO key on it leads out",
                    name_of(names, to),
                    keycode::decode_keycode(k.code)
                ),
                at: Some((k.layer, k.row, k.col)),
                layer: None,
                clearable: false,
            });
        }
    }

    for k in input.keys {
        let at = Some((k.layer, k.row, k.col));
        let place = format!("{} R{}C{}", name_of(names, k.layer), k.row, k.col);
        if let (Some(slot), Some(macros)) = (is_macro(k.code), input.macros) {
            if !macros.contains(&slot) {
                out.push(Finding {
                    level: Level::Warning,
                    text: format!("{place}: M{slot} has no macro behind it, the key does nothing"),
                    at,
                    layer: None,
                    clearable: true,
                });
            }
        }
        if let (Some(td), Some(dances)) = (is_tap_dance(k.code), input.tap_dances) {
            if !dances.contains(&td) {
                out.push(Finding {
                    level: Level::Warning,
                    text: format!("{place}: TD {td} is empty, the key does nothing"),
                    at,
                    layer: None,
                    clearable: true,
                });
            }
        }
        if k.code == SEC_CONFIRM && k.on_right_half {
            out.push(Finding {
                level: Level::Warning,
                text: format!("{place}: Sec Confirm on the right half is ignored by the firmware"),
                at,
                layer: None,
                clearable: false,
            });
        }
        if k.code == INTERNAL_WPM {
            out.push(Finding {
                level: Level::Note,
                text: format!("{place}: WPM is an internal code, the key does nothing"),
                at,
                layer: None,
                clearable: true,
            });
        }
        if k.layer == 0 && k.code == 0 {
            out.push(Finding {
                level: Level::Note,
                text: format!("{place}: empty on the base layer, the key does nothing"),
                at,
                layer: None,
                clearable: false,
            });
        }
    }
    out
}

/// Keys wearing this exact code, on the layers read so far.
pub fn used_on(keys: &[Key], code: u16) -> Vec<(usize, usize, usize)> {
    keys.iter().filter(|k| k.code == code).map(|k| (k.layer, k.row, k.col)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(layer: usize, row: usize, col: usize, code: u16) -> Key {
        Key { layer, row, col, code, on_right_half: false }
    }

    fn names(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("L{i}")).collect()
    }

    fn check(keys: &[Key], layers: usize) -> Vec<Finding> {
        run(&Input { keys, layer_names: &names(layers), macros: None, tap_dances: None })
    }

    #[test]
    fn a_healthy_keymap_has_nothing_to_say() {
        // MO 1 on the base layer reaches layer 1.
        let keys = [key(0, 0, 0, 0x04), key(0, 0, 1, 0x0200), key(1, 0, 0, 0x05)];
        assert!(check(&keys, 2).is_empty());
    }

    #[test]
    fn a_layer_with_keys_that_nothing_reaches_is_reported() {
        let keys = [key(0, 0, 0, 0x04), key(2, 0, 0, 0x05)];
        let f = check(&keys, 3);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].level, Level::Warning);
        assert_eq!(f[0].layer, Some(2));
    }

    #[test]
    fn an_empty_unreached_layer_is_not_a_problem() {
        let keys = [key(0, 0, 0, 0x04), key(5, 0, 0, 0)];
        assert!(check(&keys, 6).is_empty());
    }

    #[test]
    fn every_kind_of_layer_key_counts_as_reaching_a_layer() {
        for code in [0x0200u16, 0x0C00, 0x3101, 0x4104, 0x7011] {
            let keys = [key(0, 0, 0, code), key(1, 0, 0, 0x0B00), key(1, 0, 1, 0x05)];
            let f = check(&keys, 2);
            assert!(!f.iter().any(|x| x.layer == Some(1)), "code {code:#06X}: {f:?}");
        }
    }

    #[test]
    fn a_layer_reached_through_another_layer_counts() {
        let keys = [key(0, 0, 0, 0x0200), key(1, 0, 0, 0x0300), key(2, 0, 0, 0x05)];
        assert!(check(&keys, 3).is_empty());
    }

    #[test]
    fn to_without_a_way_back_is_a_trap() {
        // TO 1 on the base layer, nothing but letters on layer 1.
        let keys = [key(0, 0, 0, 0x0C00), key(1, 0, 0, 0x05)];
        let f = check(&keys, 2);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].at, Some((0, 0, 0)));
    }

    #[test]
    fn an_mo_is_no_way_out_of_a_to_layer_but_another_to_is() {
        let trapped = [key(0, 0, 0, 0x0C00), key(1, 0, 0, 0x0300), key(2, 0, 0, 0x05)];
        assert_eq!(check(&trapped, 3).iter().filter(|f| f.at.is_some()).count(), 1);
        let fine = [key(0, 0, 0, 0x0C00), key(1, 0, 0, 0x0C00)];
        assert!(check(&fine, 2).is_empty());
    }

    #[test]
    fn a_macro_key_without_a_macro_is_reported_once_the_list_is_known() {
        let keys = [key(0, 0, 0, 0x1500), key(0, 0, 1, 0x1600)];
        let names = names(1);
        let f = run(&Input { keys: &keys, layer_names: &names, macros: Some(&[0]), tap_dances: None });
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].at, Some((0, 0, 1)));
        // Before the list is read, nothing can be said.
        assert!(check(&keys, 1).is_empty());
    }

    #[test]
    fn an_empty_tap_dance_is_reported() {
        let keys = [key(0, 0, 0, 0x6000), key(0, 0, 1, 0x6300)];
        let names = names(1);
        let f = run(&Input { keys: &keys, layer_names: &names, macros: None, tap_dances: Some(&[0]) });
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].at, Some((0, 0, 1)));
    }

    #[test]
    fn sec_confirm_matters_only_on_the_right_half_and_only_that_exact_code() {
        let mut left = key(1, 0, 0, 0x3E00);
        left.on_right_half = false;
        let mut right = key(1, 0, 1, 0x3E00);
        right.on_right_half = true;
        let mut chest = key(1, 0, 2, 0x3E01);
        chest.on_right_half = true;
        let keys = [key(0, 0, 0, 0x0200), left, right, chest];
        let f = check(&keys, 2);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].at, Some((1, 0, 1)));
    }

    #[test]
    fn the_internal_wpm_code_is_a_note_not_a_warning() {
        let f = check(&[key(0, 0, 0, 0x3B00)], 1);
        assert_eq!(f[0].level, Level::Note);
    }

    #[test]
    fn only_keys_that_truly_do_nothing_offer_to_be_cleared() {
        let f = check(&[key(0, 0, 0, 0x3B00)], 1);
        assert!(f[0].clearable);
        // A Sec Confirm in the wrong place is a mistake to move, not to erase.
        let mut sec = key(0, 0, 1, 0x3E00);
        sec.on_right_half = true;
        assert!(!check(&[sec], 1)[0].clearable);
    }

    #[test]
    fn empty_keys_are_only_mentioned_on_the_base_layer() {
        let keys = [key(0, 0, 0, 0), key(0, 0, 1, 0x0200), key(1, 0, 0, 0)];
        let f = check(&keys, 2);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].at, Some((0, 0, 0)));
    }

    #[test]
    fn used_on_lists_every_place_of_a_code() {
        let keys = [key(0, 0, 0, 0x1500), key(2, 1, 1, 0x1500), key(0, 0, 1, 0x1600)];
        assert_eq!(used_on(&keys, 0x1500), vec![(0, 0, 0), (2, 1, 1)]);
    }
}
