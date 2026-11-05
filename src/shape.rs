//! Geometry of the physical layout: fitting it on screen, finding the key under
//! the pointer, and the edits of the shape editor. Positions are stored as the
//! protocol's `KeycapPos`: pixels at 50 px per unit, rotated around the key centre.

use crate::protocol::layout::{KeycapPos, UNIT_PX};
use egui::{pos2, vec2, Pos2, Rect, Vec2};

/// Maps layout pixels to the screen.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub origin: Pos2,
    pub scale: f32,
}

impl View {
    /// Largest scale that fits, centred, with 5% of air: same rule as the original.
    pub fn fit(keys: &[KeycapPos], area: Rect) -> Self {
        let (w, h) = content_size(keys);
        let scale = ((area.width() / w.max(1.0)).min(area.height() / h.max(1.0)) * 0.95).max(0.01);
        let origin = pos2(
            area.left() + (area.width() - w * scale) / 2.0,
            area.top() + (area.height() - h * scale) / 2.0,
        );
        Self { origin, scale }
    }

    pub fn to_screen(self, x: f32, y: f32) -> Pos2 {
        self.origin + vec2(x, y) * self.scale
    }

    pub fn key_rect(&self, k: &KeycapPos) -> Rect {
        Rect::from_min_size(self.to_screen(k.x, k.y), vec2(k.w, k.h) * self.scale)
    }
}

pub fn content_size(keys: &[KeycapPos]) -> (f32, f32) {
    keys.iter().fold((0.0f32, 0.0f32), |(w, h), k| (w.max(k.x + k.w), h.max(k.y + k.h)))
}

/// Is `p` (screen) inside this key, once its rotation is undone?
pub fn hits(view: &View, k: &KeycapPos, p: Pos2) -> bool {
    let rect = view.key_rect(k);
    let local = rotate_about(p, rect.center(), -k.angle.to_radians());
    rect.contains(local)
}

pub fn rotate_about(p: Pos2, pivot: Pos2, rad: f32) -> Pos2 {
    pivot + egui::emath::Rot2::from_angle(rad) * (p - pivot)
}

/// Topmost key under the pointer.
pub fn key_at(view: &View, keys: &[KeycapPos], p: Pos2) -> Option<usize> {
    keys.iter().rposition(|k| hits(view, k, p))
}

/// Keys whose centre lies in a selection box (screen).
pub fn keys_in_box(view: &View, keys: &[KeycapPos], area: Rect) -> Vec<usize> {
    keys.iter()
        .enumerate()
        .filter(|(_, k)| area.contains(view.key_rect(k).center()))
        .map(|(i, _)| i)
        .collect()
}

/// Key to reach from `from` with an arrow key: nearest centre in that direction,
/// favouring keys straight ahead.
pub fn neighbour(keys: &[KeycapPos], from: usize, dir: Vec2) -> Option<usize> {
    let c = |k: &KeycapPos| vec2(k.x + k.w / 2.0, k.y + k.h / 2.0);
    let start = c(keys.get(from)?);
    keys.iter()
        .enumerate()
        .filter(|&(i, _)| i != from)
        .filter_map(|(i, k)| {
            let d = c(k) - start;
            let ahead = d.dot(dir);
            if ahead <= 1.0 {
                return None;
            }
            let side = (d - dir * ahead).length();
            Some((i, ahead + side * 2.0))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

pub fn snap(v: f32, step_u: f32) -> f32 {
    if step_u <= 0.0 {
        return v;
    }
    let step = step_u * UNIT_PX;
    (v / step).round() * step
}

/// Move keys from their position at the start of a drag. The first selected key
/// snaps to the grid and the others keep their distance to it.
pub fn drag_move(
    start: &[KeycapPos],
    keys: &mut [KeycapPos],
    selection: &[usize],
    dx: f32,
    dy: f32,
    step_u: f32,
) {
    let Some(&lead) = selection.first() else { return };
    let Some(lead_key) = start.get(lead) else { return };
    let ox = snap(lead_key.x + dx, step_u) - lead_key.x;
    let oy = snap(lead_key.y + dy, step_u) - lead_key.y;
    for &i in selection {
        if let (Some(k), Some(s)) = (keys.get_mut(i), start.get(i)) {
            k.x = s.x + ox;
            k.y = s.y + oy;
        }
    }
}

/// Centre of a group of keys, in layout pixels.
pub fn group_center(keys: &[KeycapPos], selection: &[usize]) -> Option<Pos2> {
    let mut n = 0.0;
    let mut sum = vec2(0.0, 0.0);
    for &i in selection {
        let k = keys.get(i)?;
        sum += vec2(k.x + k.w / 2.0, k.y + k.h / 2.0);
        n += 1.0;
    }
    (n > 0.0).then(|| Pos2::ZERO + sum / n)
}

/// Rotate a group as one piece around `pivot`, from its state at the drag start.
pub fn drag_rotate(
    start: &[KeycapPos],
    keys: &mut [KeycapPos],
    selection: &[usize],
    pivot: Pos2,
    degrees: f32,
) {
    for &i in selection {
        if let (Some(k), Some(s)) = (keys.get_mut(i), start.get(i)) {
            let center = pos2(s.x + s.w / 2.0, s.y + s.h / 2.0);
            let moved = rotate_about(center, pivot, degrees.to_radians());
            k.x = moved.x - s.w / 2.0;
            k.y = moved.y - s.h / 2.0;
            k.angle = s.angle + degrees;
        }
    }
}

/// Mirrored copies of the selected keys, across the vertical axis in the middle
/// of the whole layout: the column is mirrored and the angle reversed.
pub fn mirrored(keys: &[KeycapPos], selection: &[usize], cols: usize) -> Vec<KeycapPos> {
    let (w, _) = content_size(keys);
    selection
        .iter()
        .filter_map(|&i| keys.get(i))
        .map(|k| KeycapPos {
            x: w - k.x - k.w,
            angle: -k.angle,
            col: cols.saturating_sub(1).saturating_sub(k.col),
            ..k.clone()
        })
        .collect()
}

/// Keep the layout's top-left corner at the origin, as parsed layouts are.
pub fn normalized_origin(keys: &mut [KeycapPos]) {
    let min_x = keys.iter().map(|k| k.x).fold(f32::INFINITY, f32::min);
    let min_y = keys.iter().map(|k| k.y).fold(f32::INFINITY, f32::min);
    if min_x.is_finite() && min_y.is_finite() {
        for k in keys.iter_mut() {
            k.x -= min_x;
            k.y -= min_y;
        }
    }
}

/// Indices of keys that claim the same matrix position as another key.
pub fn duplicate_positions(keys: &[KeycapPos]) -> Vec<usize> {
    keys.iter()
        .enumerate()
        .filter(|(i, k)| {
            keys.iter()
                .enumerate()
                .any(|(j, o)| j != *i && o.row == k.row && o.col == k.col)
        })
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::layout::default_layout;

    fn key(row: usize, col: usize, x_u: f32, y_u: f32, angle: f32) -> KeycapPos {
        KeycapPos {
            row,
            col,
            x: x_u * UNIT_PX,
            y: y_u * UNIT_PX,
            w: UNIT_PX,
            h: UNIT_PX,
            angle,
        }
    }

    fn unit_view() -> View {
        View { origin: Pos2::ZERO, scale: 1.0 }
    }

    #[test]
    fn fit_keeps_the_whole_layout_inside_the_area() {
        let keys = default_layout();
        let area = Rect::from_min_size(pos2(10.0, 20.0), vec2(800.0, 300.0));
        let v = View::fit(&keys, area);
        for k in &keys {
            assert!(area.contains_rect(v.key_rect(k).shrink(0.01)), "key R{}C{}", k.row, k.col);
        }
    }

    #[test]
    fn a_rotated_key_is_hit_by_its_rotated_corner_not_by_its_axis_corner() {
        let k = key(0, 0, 2.0, 2.0, 45.0);
        let v = unit_view();
        let r = v.key_rect(&k);
        // The axis-aligned corner falls outside a square rotated by 45 degrees...
        assert!(!hits(&v, &k, r.left_top() + vec2(1.0, 1.0)));
        // ...and the rotated square reaches beyond the sides of its axis box.
        assert!(hits(&v, &k, r.center() + vec2(UNIT_PX * 0.68, 0.0)));
        assert!(hits(&v, &k, r.center()));
    }

    #[test]
    fn key_at_returns_the_topmost_key() {
        let keys = vec![key(0, 0, 0.0, 0.0, 0.0), key(0, 1, 0.0, 0.0, 0.0)];
        assert_eq!(key_at(&unit_view(), &keys, pos2(5.0, 5.0)), Some(1));
        assert_eq!(key_at(&unit_view(), &keys, pos2(500.0, 5.0)), None);
    }

    #[test]
    fn box_selection_uses_key_centres() {
        let keys = vec![key(0, 0, 0.0, 0.0, 0.0), key(0, 1, 2.0, 0.0, 0.0)];
        let area = Rect::from_min_max(pos2(0.0, 0.0), pos2(30.0, 30.0));
        assert_eq!(keys_in_box(&unit_view(), &keys, area), vec![0]);
    }

    #[test]
    fn arrow_navigation_prefers_the_key_straight_ahead() {
        let keys = vec![
            key(0, 0, 0.0, 0.0, 0.0),
            key(0, 1, 1.0, 0.0, 0.0),
            key(1, 0, 0.0, 1.0, 0.0),
            key(1, 1, 1.0, 1.0, 0.0),
        ];
        assert_eq!(neighbour(&keys, 0, vec2(1.0, 0.0)), Some(1));
        assert_eq!(neighbour(&keys, 0, vec2(0.0, 1.0)), Some(2));
        assert_eq!(neighbour(&keys, 3, vec2(-1.0, 0.0)), Some(2));
        assert_eq!(neighbour(&keys, 0, vec2(-1.0, 0.0)), None);
    }

    #[test]
    fn drag_snaps_the_lead_key_and_keeps_the_group_shape() {
        let start = vec![key(0, 0, 0.0, 0.0, 0.0), key(0, 1, 1.1, 0.0, 0.0)];
        let mut keys = start.clone();
        // 0.3u right, snapped to a 0.25u grid.
        drag_move(&start, &mut keys, &[0, 1], 0.3 * UNIT_PX, 0.0, 0.25);
        assert!((keys[0].x - 0.25 * UNIT_PX).abs() < 0.01);
        assert!(((keys[1].x - keys[0].x) - 1.1 * UNIT_PX).abs() < 0.01);
    }

    #[test]
    fn snap_off_keeps_the_exact_value() {
        assert_eq!(snap(13.37, 0.0), 13.37);
    }

    #[test]
    fn group_rotation_moves_keys_around_the_pivot_and_turns_them() {
        let start = vec![key(0, 0, 0.0, 0.0, 10.0)];
        let mut keys = start.clone();
        let center = pos2(25.0, 25.0);
        let pivot = pos2(125.0, 25.0);
        drag_rotate(&start, &mut keys, &[0], pivot, 90.0);
        let c = pos2(keys[0].x + 25.0, keys[0].y + 25.0);
        let expected = rotate_about(center, pivot, 90f32.to_radians());
        assert!((c - expected).length() < 0.01);
        assert!((keys[0].angle - 100.0).abs() < 0.01);
    }

    #[test]
    fn mirror_reverses_position_angle_and_column() {
        let keys = vec![key(0, 0, 0.0, 1.0, 10.0), key(0, 12, 9.0, 0.0, 0.0)];
        let m = mirrored(&keys, &[0], 13);
        assert_eq!(m.len(), 1);
        assert!((m[0].x - 9.0 * UNIT_PX).abs() < 0.01);
        assert!((m[0].y - 1.0 * UNIT_PX).abs() < 0.01);
        assert_eq!(m[0].angle, -10.0);
        assert_eq!(m[0].col, 12);
    }

    #[test]
    fn duplicates_flag_every_key_sharing_a_matrix_position() {
        let keys = vec![key(0, 0, 0.0, 0.0, 0.0), key(0, 1, 1.0, 0.0, 0.0), key(0, 0, 2.0, 0.0, 0.0)];
        assert_eq!(duplicate_positions(&keys), vec![0, 2]);
    }

    #[test]
    fn the_default_layout_has_no_duplicate_matrix_position() {
        assert!(duplicate_positions(&default_layout()).is_empty());
    }

    #[test]
    fn normalizing_brings_the_corner_back_to_the_origin() {
        let mut keys = vec![key(0, 0, 1.0, 2.0, 0.0), key(0, 1, 3.0, 4.0, 0.0)];
        normalized_origin(&mut keys);
        assert_eq!((keys[0].x, keys[0].y), (0.0, 0.0));
        assert!((keys[1].x - 2.0 * UNIT_PX).abs() < 0.01);
    }
}
