//! The Layout tab: edit the physical shape of a keyboard by hand. Keys move with
//! the mouse, turn with a handle, and everything can also be typed in. This
//! buffer stays apart from the keyboard on the Keymap tab until the person asks;
//! the protocol has no command to write a layout, so the result leaves as a file.

use crate::app::App;
use crate::backend::PickTarget;
use crate::protocol::layout::{self, KeycapPos, UNIT_PX};
use crate::shape::{self, View};
use crate::theme;
use crate::ui_common::{note, paint_key, section, CapStyle};
use egui::{pos2, vec2, Frame, Key, Modifiers, Pos2, Rect, RichText, Sense, Stroke};

/// Grid steps offered for moving keys, in key units (0 = free).
pub const SNAP_STEPS: [(f32, &str); 5] = [(0.0, "Free"), (0.05, "0.05 u"), (0.25, "0.25 u"), (0.5, "0.5 u"), (1.0, "1 u")];
const ROTATE_SNAP_DEG: f32 = 5.0;
const HANDLE_GAP: f32 = 26.0;
const HANDLE_R: f32 = 7.0;
const FIELD_MERGE_S: f64 = 0.6;

enum Gesture {
    Move { start: Vec<KeycapPos>, from: Pos2 },
    Rotate { start: Vec<KeycapPos>, pivot: Pos2, from_deg: f32 },
    Box { from: Pos2 },
}

pub struct LayoutEditor {
    pub keys: Vec<KeycapPos>,
    pub selection: Vec<usize>,
    pub status: String,
    pub file_path: String,
    pub auto_refresh: bool,
    pub dirty: bool,
    pub snap: usize,
    pub cols: usize,
    pub show_json: bool,
    undo: Vec<Vec<KeycapPos>>,
    redo: Vec<Vec<KeycapPos>>,
    gesture: Option<Gesture>,
    frozen: Option<View>,
    last_field_edit: f64,
    json: String,
}

impl LayoutEditor {
    pub fn new(keys: Vec<KeycapPos>) -> Self {
        let json = layout::serialize_json(&keys);
        Self {
            keys,
            selection: Vec::new(),
            status: String::new(),
            file_path: String::new(),
            auto_refresh: false,
            dirty: false,
            snap: 2,
            cols: 13,
            show_json: false,
            undo: Vec::new(),
            redo: Vec::new(),
            gesture: None,
            frozen: None,
            last_field_edit: -10.0,
            json,
        }
    }

    pub fn json(&self) -> &str {
        &self.json
    }

    fn step(&self) -> f32 {
        SNAP_STEPS[self.snap].0
    }

    fn touched(&mut self) {
        self.json = layout::serialize_json(&self.keys);
        self.dirty = true;
        self.status = "Layout edited — Export to save".into();
    }

    fn checkpoint(&mut self) {
        self.undo.push(self.keys.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Replace the buffer with keys from elsewhere (not an edit).
    pub fn set_keys(&mut self, keys: Vec<KeycapPos>) {
        self.keys = keys;
        self.selection.clear();
        self.undo.clear();
        self.redo.clear();
        self.gesture = None;
        self.frozen = None;
        self.dirty = false;
        self.json = layout::serialize_json(&self.keys);
    }

    pub fn load_json(&mut self, json: &str) {
        match layout::parse_json(json) {
            Ok(keys) => {
                self.status = format!("{} keys loaded", keys.len());
                if let Some((_, cols)) = layout::parse_geometry(json) {
                    self.cols = cols;
                }
                self.set_keys(keys);
            }
            Err(e) => self.status = format!("Parse error: {e}"),
        }
    }

    fn undo_step(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.keys, prev));
            self.selection.retain(|&i| i < self.keys.len());
            self.touched();
        }
    }

    fn redo_step(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.keys, next));
            self.selection.retain(|&i| i < self.keys.len());
            self.touched();
        }
    }

    fn add_key(&mut self) {
        self.checkpoint();
        self.keys.push(KeycapPos { row: 0, col: 0, x: 0.0, y: 0.0, w: UNIT_PX, h: UNIT_PX, angle: 0.0 });
        self.selection = vec![self.keys.len() - 1];
        self.touched();
        self.status = "Key added — Export to save".into();
    }

    fn delete_selected(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        let mut gone = std::mem::take(&mut self.selection);
        gone.sort_unstable();
        for i in gone.into_iter().rev() {
            if i < self.keys.len() {
                self.keys.remove(i);
            }
        }
        self.touched();
        self.status = "Key deleted — Export to save".into();
    }

    fn duplicate_selected(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        let start = self.keys.len();
        let copies: Vec<KeycapPos> = self
            .selection
            .iter()
            .filter_map(|&i| self.keys.get(i))
            .map(|k| KeycapPos { x: k.x + 0.25 * UNIT_PX, y: k.y + 0.25 * UNIT_PX, ..k.clone() })
            .collect();
        self.keys.extend(copies);
        self.selection = (start..self.keys.len()).collect();
        self.touched();
    }

    fn mirror_selected(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        let start = self.keys.len();
        let copies = shape::mirrored(&self.keys, &self.selection, self.cols);
        self.keys.extend(copies);
        self.selection = (start..self.keys.len()).collect();
        self.touched();
    }

    fn nudge(&mut self, dx: f32, dy: f32) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        for &i in &self.selection {
            if let Some(k) = self.keys.get_mut(i) {
                k.x += dx;
                k.y += dy;
            }
        }
        self.settle();
        self.touched();
    }

    /// Bring keys that went past the top or left edge back into the positive area.
    fn settle(&mut self) {
        if self.keys.iter().any(|k| k.x < 0.0 || k.y < 0.0) {
            shape::normalized_origin(&mut self.keys);
        }
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let margin = Frame::new().inner_margin(egui::Margin::same(12));
    egui::Panel::top("layout_head")
        .frame(margin)
        .show_separator_line(false)
        .show(ui, |ui| header(app, ui));
    let ed_json = app.layout_ed.show_json;
    egui::Panel::bottom("layout_json")
        .frame(Frame::new().inner_margin(egui::Margin { left: 12, right: 12, top: 0, bottom: 10 }))
        .show_separator_line(false)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut app.layout_ed.show_json, "Show JSON");
                let dups = shape::duplicate_positions(&app.layout_ed.keys);
                if !dups.is_empty() {
                    ui.label(
                        RichText::new(format!("{} keys share a matrix position (red)", dups.len()))
                            .color(theme::RED)
                            .size(11.0),
                    );
                }
            });
            if ed_json {
                Frame::new().fill(theme::BG_SURFACE).corner_radius(4).inner_margin(6).show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(110.0).auto_shrink([false, true]).show(ui, |ui| {
                        ui.label(RichText::new(app.layout_ed.json()).monospace().size(10.0));
                    });
                });
            }
        });
    egui::Panel::right("layout_side")
        .exact_size(196.0)
        .frame(Frame::new().inner_margin(egui::Margin { left: 0, right: 12, top: 0, bottom: 8 }))
        .show_separator_line(false)
        .show(ui, |ui| inspector(&mut app.layout_ed, ui));
    egui::CentralPanel::default()
        .frame(Frame::new().inner_margin(egui::Margin { left: 12, right: 10, top: 0, bottom: 8 }))
        .show(ui, |ui| canvas(&mut app.layout_ed, ui));
    if !app.any_dialog_open() && !ui.ctx().egui_wants_keyboard_input() {
        shortcuts(&mut app.layout_ed, ui);
    }
}

fn header(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        section(ui, "Keyboard shape");
        let chosen = app.layout_board;
        let current = app.boards.get(chosen).map(|b| b.name.clone()).unwrap_or_default();
        let mut picked = None;
        egui::ComboBox::from_id_salt("layout_board").width(150.0).selected_text(current).show_ui(ui, |ui| {
            for (i, b) in app.boards.iter().enumerate() {
                if ui.selectable_label(i == chosen, &b.name).clicked() {
                    picked = Some(i);
                }
            }
        });
        if let Some(i) = picked {
            app.layout_board = i;
            let b = &app.boards[i];
            app.layout_ed.set_keys(b.keys.clone());
            app.layout_ed.cols = b.cols;
            app.layout_ed.status = format!("{} keys from the firmware's {}", b.keys.len(), b.name);
        }
        ui.add_enabled_ui(app.online(), |ui| {
            if ui.add(crate::ui_common::primary("From keyboard")).clicked() {
                app.be.load_layout_from_keyboard();
            }
        });
        if ui.button("Load file...").clicked() {
            app.be.pick(PickTarget::LayoutLoad);
        }
        if ui.button("Export...").clicked() {
            app.be.pick(PickTarget::LayoutExport);
        }
        if ui
            .button("Use on Keymap tab")
            .on_hover_text("Draws the Keymap tab with this shape. The keyboard itself keeps its own: a layout can only leave as a file.")
            .clicked()
        {
            app.keys = app.layout_ed.keys.clone();
            app.selected = None;
            app.layout_ed.status = "Keymap tab now uses this shape".into();
        }
    });
    if !app.layout_ed.file_path.is_empty() {
        ui.horizontal(|ui| {
            ui.checkbox(&mut app.layout_ed.auto_refresh, "Auto-refresh (5s)");
            note(ui, &app.layout_ed.file_path);
        });
    }
    if !app.layout_ed.status.is_empty() {
        note(ui, &app.layout_ed.status);
    }
}

fn selection_bounds(view: &View, ed: &LayoutEditor) -> Option<Rect> {
    let mut out: Option<Rect> = None;
    for &i in &ed.selection {
        let r = view.key_rect(ed.keys.get(i)?);
        // Rotated corners reach beyond the axis box: cover the circle around the key.
        let reach = r.size().length() / 2.0;
        let c = Rect::from_center_size(r.center(), vec2(reach * 2.0, reach * 2.0));
        out = Some(out.map_or(c, |o| o.union(c)));
    }
    out
}

fn handle_pos(view: &View, ed: &LayoutEditor) -> Option<Pos2> {
    let b = selection_bounds(view, ed)?;
    Some(pos2(b.center().x, b.top() - HANDLE_GAP / 2.0))
}

fn canvas(ed: &mut LayoutEditor, ui: &mut egui::Ui) {
    let (area, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    ui.painter().rect_filled(area, 8.0, theme::BG_SURFACE);
    let painter = ui.painter_at(area);
    if ed.keys.is_empty() {
        painter.text(
            area.center(),
            egui::Align2::CENTER_CENTER,
            "No layout loaded",
            egui::FontId::proportional(13.0),
            theme::FG_MUTED,
        );
    }
    // The picture must not rescale under the pointer during a gesture.
    let view = ed.frozen.unwrap_or_else(|| View::fit(&ed.keys, area.shrink(14.0)));

    // One-unit grid.
    let step = UNIT_PX * view.scale;
    if step > 6.0 {
        let grid = theme::BG_SECONDARY.gamma_multiply(0.35);
        let mut x = view.origin.x.rem_euclid(step) + area.left() - area.left().rem_euclid(step);
        while x < area.right() {
            painter.vline(x, area.y_range(), Stroke::new(1.0, grid));
            x += step;
        }
        let mut y = view.origin.y.rem_euclid(step) + area.top() - area.top().rem_euclid(step);
        while y < area.bottom() {
            painter.hline(area.x_range(), y, Stroke::new(1.0, grid));
            y += step;
        }
    }

    let dups = shape::duplicate_positions(&ed.keys);
    let pointer = ui.input(|i| i.pointer.latest_pos());
    let hovered = pointer.filter(|p| area.contains(*p)).and_then(|p| shape::key_at(&view, &ed.keys, p));
    for (i, k) in ed.keys.iter().enumerate() {
        let selected = ed.selection.contains(&i);
        let stroke = if dups.contains(&i) {
            Stroke::new(2.0, theme::RED)
        } else if selected {
            Stroke::new(2.0, theme::CYAN)
        } else {
            Stroke::new(1.0, theme::BG_PRIMARY)
        };
        let mut fill = if selected { egui::Color32::from_rgb(0x5b, 0x5f, 0x7a) } else { theme::KEYCAP };
        if hovered == Some(i) && !selected {
            fill = fill.gamma_multiply(1.2);
        }
        let style = CapStyle { fill, stroke, text: theme::FG_PRIMARY };
        paint_key(&painter, &view, k, &style, &format!("R{}C{}", k.row, k.col));
    }

    let handle = handle_pos(&view, ed);
    if let (Some(h), Some(b)) = (handle, selection_bounds(&view, ed)) {
        painter.line_segment([pos2(h.x, b.top()), h], Stroke::new(1.0, theme::CYAN));
        painter.circle_filled(h, HANDLE_R, theme::CYAN);
    }

    let press = ui.input(|i| i.pointer.press_origin());
    if resp.drag_started() {
        if let Some(origin) = press {
            let on_handle = handle.is_some_and(|h| (origin - h).length() <= HANDLE_R + 3.0);
            if on_handle {
                if let Some(c) = shape::group_center(&ed.keys, &ed.selection) {
                    let screen = view.to_screen(c.x, c.y);
                    ed.checkpoint();
                    ed.frozen = Some(view);
                    ed.gesture = Some(Gesture::Rotate {
                        start: ed.keys.clone(),
                        pivot: c,
                        from_deg: (origin - screen).angle().to_degrees(),
                    });
                }
            } else if let Some(i) = shape::key_at(&view, &ed.keys, origin) {
                if !ed.selection.contains(&i) {
                    ed.selection = vec![i];
                } else {
                    // The key under the pointer leads, so it is the one that lands on the grid.
                    ed.selection.retain(|&s| s != i);
                    ed.selection.insert(0, i);
                }
                ed.checkpoint();
                ed.frozen = Some(view);
                ed.gesture = Some(Gesture::Move { start: ed.keys.clone(), from: origin });
            } else {
                ed.frozen = Some(view);
                ed.gesture = Some(Gesture::Box { from: origin });
            }
        }
    }

    if let (Some(g), Some(p)) = (&ed.gesture, pointer) {
        match g {
            Gesture::Move { start, from } => {
                let d = (p - *from) / view.scale;
                let selection = ed.selection.clone();
                let step = SNAP_STEPS[ed.snap].0;
                shape::drag_move(start, &mut ed.keys, &selection, d.x, d.y, step);
            }
            Gesture::Rotate { start, pivot, from_deg } => {
                let screen = view.to_screen(pivot.x, pivot.y);
                let now = (p - screen).angle().to_degrees();
                let mut deg = now - from_deg;
                if ed.step() > 0.0 {
                    deg = (deg / ROTATE_SNAP_DEG).round() * ROTATE_SNAP_DEG;
                }
                let selection = ed.selection.clone();
                shape::drag_rotate(start, &mut ed.keys, &selection, *pivot, deg);
            }
            Gesture::Box { from } => {
                let r = Rect::from_two_pos(*from, p);
                painter.rect_filled(r, 2.0, theme::CYAN.gamma_multiply(0.12));
                painter.rect_stroke(r, 2.0, Stroke::new(1.0, theme::CYAN), egui::StrokeKind::Inside);
            }
        }
    }

    if resp.drag_stopped() {
        if let Some(g) = ed.gesture.take() {
            match g {
                Gesture::Box { from } => {
                    if let Some(p) = pointer {
                        ed.selection = shape::keys_in_box(&view, &ed.keys, Rect::from_two_pos(from, p));
                    }
                }
                Gesture::Move { start, .. } | Gesture::Rotate { start, .. } => {
                    if start == ed.keys {
                        // Nothing moved: don't leave an empty undo step.
                        ed.undo.pop();
                    } else {
                        ed.settle();
                        ed.touched();
                    }
                }
            }
        }
        ed.frozen = None;
    }

    if resp.clicked() {
        let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        match (hovered, add) {
            (Some(i), true) => {
                if let Some(at) = ed.selection.iter().position(|&s| s == i) {
                    ed.selection.remove(at);
                } else {
                    ed.selection.push(i);
                }
            }
            (Some(i), false) => ed.selection = vec![i],
            (None, false) => ed.selection.clear(),
            (None, true) => {}
        }
    }
}

fn inspector(ed: &mut LayoutEditor, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 5.0);
        ui.spacing_mut().button_padding = vec2(8.0, 4.0);
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!ed.undo.is_empty(), egui::Button::new("↶ Undo")).clicked() {
                ed.undo_step();
            }
            if ui.add_enabled(!ed.redo.is_empty(), egui::Button::new("↷ Redo")).clicked() {
                ed.redo_step();
            }
            if ui.button("Add key").clicked() {
                ed.add_key();
            }
            let has = !ed.selection.is_empty();
            if ui.add_enabled(has, egui::Button::new("Delete")).clicked() {
                ed.delete_selected();
            }
            if ui.add_enabled(has, egui::Button::new("Duplicate")).clicked() {
                ed.duplicate_selected();
            }
            if ui
                .add_enabled(has, egui::Button::new("Mirror"))
                .on_hover_text("Adds a copy on the other side of the keyboard, with the column and the angle reversed")
                .clicked()
            {
                ed.mirror_selected();
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Grid").size(11.0).color(theme::FG_MUTED));
            let mut snap = ed.snap;
            egui::ComboBox::from_id_salt("snap")
                .width(90.0)
                .show_index(ui, &mut snap, SNAP_STEPS.len(), |i| SNAP_STEPS[i].1);
            ed.snap = snap;
        });
        ui.add_space(4.0);
        ui.separator();
        match ed.selection.len() {
            0 => note(ui, "Click a key to edit, drag it to move it, drag on the background to select several."),
            1 => note(ui, &format!("Editing key #{}", ed.selection[0])),
            n => note(ui, &format!("{n} keys: moves and turns apply to all")),
        }
        if let Some(&lead) = ed.selection.first() {
            fields(ed, ui, lead);
            note(ui, "Drag the round handle to turn. Arrows nudge by the grid step.");
        }
    });
}

fn fields(ed: &mut LayoutEditor, ui: &mut egui::Ui, lead: usize) {
    let Some(k) = ed.keys.get(lead).cloned() else { return };
    let multi = ed.selection.len() > 1;
    let (mut x, mut y, mut r) = (k.x / UNIT_PX, k.y / UNIT_PX, k.angle);
    let (mut w, mut h) = (k.w / UNIT_PX, k.h / UNIT_PX);
    let (mut row, mut col) = (k.row, k.col);
    let mut changed = false;
    egui::Grid::new("key_fields").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
        let mut unit = |ui: &mut egui::Ui, label: &str, v: &mut f32, speed: f64, suffix: &str| {
            ui.label(RichText::new(label).size(11.0).color(theme::FG_MUTED));
            changed |= ui.add(egui::DragValue::new(v).speed(speed).max_decimals(2).suffix(suffix)).changed();
            ui.end_row();
        };
        unit(ui, "X", &mut x, 0.01, " u");
        unit(ui, "Y", &mut y, 0.01, " u");
        unit(ui, "Rotation", &mut r, 0.5, "°");
        unit(ui, "Width", &mut w, 0.01, " u");
        unit(ui, "Height", &mut h, 0.01, " u");
        if !multi {
            ui.label(RichText::new("Row").size(11.0).color(theme::FG_MUTED));
            changed |= ui.add(egui::DragValue::new(&mut row).range(0..=31)).changed();
            ui.end_row();
            ui.label(RichText::new("Col").size(11.0).color(theme::FG_MUTED));
            changed |= ui.add(egui::DragValue::new(&mut col).range(0..=31)).changed();
            ui.end_row();
        }
    });
    if !changed {
        return;
    }
    // A slow drag on a field is one undo step, not hundreds.
    let now = ui.input(|i| i.time);
    if now - ed.last_field_edit > FIELD_MERGE_S {
        ed.checkpoint();
    }
    ed.last_field_edit = now;
    let (dx, dy, dr) = (x * UNIT_PX - k.x, y * UNIT_PX - k.y, r - k.angle);
    let (nw, nh) = ((w.max(0.25)) * UNIT_PX, (h.max(0.25)) * UNIT_PX);
    let selection = ed.selection.clone();
    for i in selection {
        if let Some(key) = ed.keys.get_mut(i) {
            key.x += dx;
            key.y += dy;
            key.angle += dr;
            key.w = nw;
            key.h = nh;
            if !multi {
                key.row = row;
                key.col = col;
            }
        }
    }
    ed.settle();
    ed.touched();
}

fn shortcuts(ed: &mut LayoutEditor, ui: &mut egui::Ui) {
    let (undo, redo, all, del, dup) = ui.input_mut(|i| {
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        (
            i.consume_key(Modifiers::COMMAND, Key::Z),
            i.consume_key(Modifiers::COMMAND, Key::Y) || i.consume_key(cmd_shift, Key::Z),
            i.consume_key(Modifiers::COMMAND, Key::A),
            i.key_pressed(Key::Delete),
            i.consume_key(Modifiers::COMMAND, Key::D),
        )
    });
    if undo {
        ed.undo_step();
    }
    if redo {
        ed.redo_step();
    }
    if all {
        ed.selection = (0..ed.keys.len()).collect();
    }
    if del {
        ed.delete_selected();
    }
    if dup {
        ed.duplicate_selected();
    }
    let step = if ed.step() > 0.0 { ed.step() } else { 0.05 } * UNIT_PX;
    for (key, dx, dy) in [
        (Key::ArrowLeft, -step, 0.0),
        (Key::ArrowRight, step, 0.0),
        (Key::ArrowUp, 0.0, -step),
        (Key::ArrowDown, 0.0, step),
    ] {
        if ui.input(|i| i.key_pressed(key)) {
            ed.nudge(dx, dy);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(col: usize, x_u: f32) -> KeycapPos {
        KeycapPos { row: 0, col, x: x_u * UNIT_PX, y: 0.0, w: UNIT_PX, h: UNIT_PX, angle: 0.0 }
    }

    fn editor() -> LayoutEditor {
        LayoutEditor::new(vec![key(0, 0.0), key(1, 1.0), key(2, 2.0)])
    }

    #[test]
    fn delete_removes_every_selected_key_and_undo_restores_them() {
        let mut e = editor();
        e.selection = vec![0, 2];
        e.delete_selected();
        assert_eq!(e.keys.len(), 1);
        assert_eq!(e.keys[0].col, 1);
        e.undo_step();
        assert_eq!(e.keys.len(), 3);
        e.redo_step();
        assert_eq!(e.keys.len(), 1);
    }

    #[test]
    fn duplicate_selects_the_copies_offset_from_the_originals() {
        let mut e = editor();
        e.selection = vec![1];
        e.duplicate_selected();
        assert_eq!(e.keys.len(), 4);
        assert_eq!(e.selection, vec![3]);
        assert!((e.keys[3].x - 1.25 * UNIT_PX).abs() < 0.01);
    }

    #[test]
    fn nudging_past_the_edge_shifts_everything_back_to_the_origin() {
        let mut e = editor();
        e.selection = vec![0];
        e.nudge(-UNIT_PX, 0.0);
        assert_eq!(e.keys[0].x, 0.0);
        assert_eq!(e.keys[1].x, 2.0 * UNIT_PX);
    }

    #[test]
    fn edits_mark_the_buffer_and_show_in_the_json() {
        let mut e = editor();
        assert!(!e.dirty);
        e.add_key();
        assert!(e.dirty);
        let parsed = layout::parse_json(e.json()).unwrap();
        assert_eq!(parsed.len(), 4);
    }

    #[test]
    fn a_bad_file_leaves_the_buffer_alone_and_says_so() {
        let mut e = editor();
        e.load_json("not json");
        assert_eq!(e.keys.len(), 3);
        assert!(e.status.starts_with("Parse error"));
    }

    #[test]
    fn loading_a_layout_takes_its_column_count_and_clears_undo() {
        let mut e = editor();
        e.add_key();
        e.load_json(r#"{"rows":4,"cols":14,"keys":[{"row":0,"col":0,"x":0,"y":0}]}"#);
        assert_eq!(e.cols, 14);
        assert_eq!(e.keys.len(), 1);
        assert!(e.undo.is_empty());
        assert!(!e.dirty);
    }
}
