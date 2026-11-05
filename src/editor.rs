//! The keymap being edited: what the keyboard holds, the edits it hasn't
//! received yet, and undo/redo. No UI and no serial port in here.
//!
//! Connected, an edit lands in `layers` and is returned for sending. Offline it
//! stays in `pending` and overlays `layers`, until the person sends or drops it.

use std::collections::BTreeMap;

/// (layer, row, col)
pub type Pos = (usize, usize, usize);

#[derive(Clone, Copy, Debug, PartialEq)]
struct Change {
    pos: Pos,
    old: Option<u16>,
    new: Option<u16>,
}

#[derive(Default)]
pub struct KeymapDoc {
    layers: BTreeMap<usize, Vec<Vec<u16>>>,
    pending: BTreeMap<Pos, u16>,
    undo: Vec<Vec<Change>>,
    redo: Vec<Vec<Change>>,
    pub clipboard: Option<u16>,
    pub layer_clipboard: Option<Vec<Vec<u16>>>,
}

const UNDO_DEPTH: usize = 200;

impl KeymapDoc {
    /// Keycode to show: an unsent edit wins over what the keyboard holds.
    /// `None` means nothing is known yet (no keyboard, layer not loaded).
    pub fn get(&self, pos: Pos) -> Option<u16> {
        let (l, r, c) = pos;
        self.pending
            .get(&pos)
            .copied()
            .or_else(|| self.layers.get(&l)?.get(r)?.get(c).copied())
    }

    /// What the keyboard itself holds, ignoring unsent edits.
    #[cfg(test)]
    pub fn stored(&self, pos: Pos) -> Option<u16> {
        let (l, r, c) = pos;
        self.layers.get(&l)?.get(r)?.get(c).copied()
    }

    pub fn is_pending(&self, pos: Pos) -> bool {
        self.pending.contains_key(&pos)
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn set_layer(&mut self, layer: usize, km: Vec<Vec<u16>>) {
        self.layers.insert(layer, km);
    }

    pub fn layer(&self, layer: usize) -> Option<&Vec<Vec<u16>>> {
        self.layers.get(&layer)
    }

    /// Forget what the keyboard held. Unsent edits survive, undo doesn't:
    /// the next keyboard may have another shape.
    pub fn forget_device(&mut self) {
        self.layers.clear();
        self.undo.clear();
        self.redo.clear();
    }

    /// What an empty key does on a higher layer. The firmware falls back one
    /// step, to the layer the key was reached from (`last_layer`); the editor
    /// can't know that at rest, so it shows the usual case: the base layer.
    pub fn fall_through(&self, pos: Pos) -> Option<u16> {
        let (l, r, c) = pos;
        if l == 0 {
            return None;
        }
        self.get((0, r, c)).filter(|&v| v != 0)
    }

    fn write(&mut self, pos: Pos, v: Option<u16>, online: bool, out: &mut Vec<(Pos, u16)>) {
        if online {
            let (l, r, c) = pos;
            let code = v.unwrap_or(0);
            self.pending.remove(&pos);
            if let Some(cell) = self.layers.get_mut(&l).and_then(|m| m.get_mut(r)).and_then(|m| m.get_mut(c)) {
                *cell = code;
                out.push((pos, code));
            } else if self.layers.contains_key(&l) {
                // Outside this keyboard's matrix: nothing to send.
            } else {
                // Layer not loaded yet: the keyboard still takes the write.
                out.push((pos, code));
            }
        } else {
            match v {
                Some(code) => {
                    self.pending.insert(pos, code);
                }
                None => {
                    self.pending.remove(&pos);
                }
            }
        }
    }

    /// Apply a group of edits as one undo step. Returns the writes to send.
    pub fn apply(&mut self, edits: &[(Pos, u16)], online: bool) -> Vec<(Pos, u16)> {
        let mut group = Vec::new();
        let mut out = Vec::new();
        for &(pos, code) in edits {
            let old = self.get(pos);
            if old == Some(code) {
                continue;
            }
            group.push(Change { pos, old, new: Some(code) });
            self.write(pos, Some(code), online, &mut out);
        }
        if !group.is_empty() {
            self.undo.push(group);
            if self.undo.len() > UNDO_DEPTH {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        out
    }

    /// Swap two keys, one undo step.
    pub fn swap(&mut self, a: Pos, b: Pos, online: bool) -> Vec<(Pos, u16)> {
        let (va, vb) = (self.get(a).unwrap_or(0), self.get(b).unwrap_or(0));
        self.apply(&[(a, vb), (b, va)], online)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self, online: bool) -> Vec<(Pos, u16)> {
        let mut out = Vec::new();
        if let Some(group) = self.undo.pop() {
            for ch in group.iter().rev() {
                self.write(ch.pos, ch.old, online, &mut out);
            }
            self.redo.push(group);
        }
        out
    }

    pub fn redo(&mut self, online: bool) -> Vec<(Pos, u16)> {
        let mut out = Vec::new();
        if let Some(group) = self.redo.pop() {
            for ch in &group {
                self.write(ch.pos, ch.new, online, &mut out);
            }
            self.undo.push(group);
        }
        out
    }

    /// Hand over unsent edits for sending; they now count as stored.
    pub fn take_pending(&mut self) -> Vec<(Pos, u16)> {
        let taken: Vec<_> = std::mem::take(&mut self.pending).into_iter().collect();
        for &((l, r, c), code) in &taken {
            if let Some(cell) = self.layers.get_mut(&l).and_then(|m| m.get_mut(r)).and_then(|m| m.get_mut(c)) {
                *cell = code;
            }
        }
        taken
    }

    pub fn discard_pending(&mut self) {
        self.pending.clear();
        self.undo.clear();
        self.redo.clear();
    }

    /// A write the keyboard refused: keep it as unsent so it isn't lost.
    pub fn mark_unsent(&mut self, pos: Pos, code: u16) {
        self.pending.insert(pos, code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded() -> KeymapDoc {
        let mut d = KeymapDoc::default();
        d.set_layer(0, vec![vec![4, 5, 6], vec![7, 8, 9]]);
        d
    }

    #[test]
    fn online_edit_updates_the_map_and_asks_to_send_it() {
        let mut d = loaded();
        let out = d.apply(&[((0, 0, 1), 0x2C)], true);
        assert_eq!(out, vec![((0, 0, 1), 0x2C)]);
        assert_eq!(d.get((0, 0, 1)), Some(0x2C));
        assert_eq!(d.pending_count(), 0);
    }

    #[test]
    fn writing_the_same_code_is_a_no_op_with_no_undo_step() {
        let mut d = loaded();
        assert!(d.apply(&[((0, 0, 1), 5)], true).is_empty());
        assert!(!d.can_undo());
    }

    #[test]
    fn offline_edit_overlays_and_sends_nothing() {
        let mut d = loaded();
        assert!(d.apply(&[((0, 0, 0), 0x2C)], false).is_empty());
        assert_eq!(d.get((0, 0, 0)), Some(0x2C));
        assert_eq!(d.stored((0, 0, 0)), Some(4));
        assert!(d.is_pending((0, 0, 0)));
    }

    #[test]
    fn swap_exchanges_both_keys_and_undoes_in_one_step() {
        let mut d = loaded();
        let out = d.swap((0, 0, 0), (0, 1, 2), true);
        assert_eq!(out.len(), 2);
        assert_eq!(d.get((0, 0, 0)), Some(9));
        assert_eq!(d.get((0, 1, 2)), Some(4));
        let back = d.undo(true);
        assert_eq!(back.len(), 2);
        assert_eq!(d.get((0, 0, 0)), Some(4));
        assert_eq!(d.get((0, 1, 2)), Some(9));
        assert!(!d.can_undo());
        d.redo(true);
        assert_eq!(d.get((0, 0, 0)), Some(9));
    }

    #[test]
    fn undo_sends_the_previous_code_to_the_keyboard() {
        let mut d = loaded();
        d.apply(&[((0, 0, 1), 0x2C)], true);
        assert_eq!(d.undo(true), vec![((0, 0, 1), 5)]);
    }

    #[test]
    fn offline_undo_on_an_unknown_key_returns_to_unknown() {
        let mut d = KeymapDoc::default();
        d.apply(&[((1, 0, 0), 0x2C)], false);
        assert_eq!(d.get((1, 0, 0)), Some(0x2C));
        d.undo(false);
        assert_eq!(d.get((1, 0, 0)), None);
        assert_eq!(d.pending_count(), 0);
    }

    #[test]
    fn a_new_edit_clears_redo() {
        let mut d = loaded();
        d.apply(&[((0, 0, 0), 1)], true);
        d.undo(true);
        assert!(d.can_redo());
        d.apply(&[((0, 0, 1), 2)], true);
        assert!(!d.can_redo());
    }

    #[test]
    fn writes_outside_the_matrix_are_not_sent() {
        let mut d = loaded();
        assert!(d.apply(&[((0, 9, 9), 1)], true).is_empty());
    }

    #[test]
    fn empty_keys_fall_back_one_step_to_the_base_layer_not_through_every_layer() {
        let mut d = loaded();
        d.set_layer(1, vec![vec![0, 0x10, 0], vec![0, 0, 0]]);
        d.set_layer(2, vec![vec![0, 0, 0], vec![0, 0, 0]]);
        // Layer 1 has a key here, but the fallback goes straight to the base layer.
        assert_eq!(d.fall_through((2, 0, 1)), Some(5));
        assert_eq!(d.fall_through((2, 0, 0)), Some(4));
        assert_eq!(d.fall_through((0, 0, 0)), None);
    }

    #[test]
    fn take_pending_empties_and_stores() {
        let mut d = loaded();
        d.apply(&[((0, 0, 0), 0x2C)], false);
        assert_eq!(d.take_pending(), vec![((0, 0, 0), 0x2C)]);
        assert_eq!(d.pending_count(), 0);
        assert_eq!(d.stored((0, 0, 0)), Some(0x2C));
    }

    #[test]
    fn forgetting_the_device_keeps_unsent_edits() {
        let mut d = loaded();
        d.apply(&[((0, 0, 0), 0x2C)], false);
        d.forget_device();
        assert_eq!(d.get((0, 0, 0)), Some(0x2C));
        assert_eq!(d.stored((0, 0, 0)), None);
        assert!(!d.can_undo());
    }
}
