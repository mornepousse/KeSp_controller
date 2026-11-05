//! The Keymap tab: the interactive keyboard, layers and key palette.

use crate::app::{Action, App, Confirm, DragItem, Target};
use crate::checks::Level;
use crate::keys::{self, SECTIONS};
use crate::protocol::layout_remap::KeyboardLayout;
use crate::shape::{self, View};
use crate::theme;
use crate::ui_common::{paint_key, CapStyle};
use egui::{vec2, Color32, Frame, Key, Modifiers, RichText, Sense, Stroke};

/// Colour of a key family on the keyboard.
fn family_fill(family: &str) -> Color32 {
    match family {
        "none" => Color32::from_rgb(0x33, 0x35, 0x47),
        "mod" => Color32::from_rgb(0x5a, 0x4a, 0x7a),
        "layer" => Color32::from_rgb(0x2f, 0x5f, 0x6b),
        "macro" => Color32::from_rgb(0x6b, 0x50, 0x3a),
        "special" => Color32::from_rgb(0x6a, 0x3f, 0x5c),
        _ => theme::KEYCAP,
    }
}

const INFO_H: f32 = 46.0;
const PALETTE_SHARE: f32 = 0.28;
const PALETTE_MIN_H: f32 = 84.0;
const PALETTE_MAX_H: f32 = 176.0;

const LEGEND: [(&str, &str); 5] = [
    ("Modifier / Mod-Tap", "mod"),
    ("Layer", "layer"),
    ("Macro / Tap Dance", "macro"),
    ("Special", "special"),
    ("Empty", "none"),
];

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let margin = Frame::new().inner_margin(8);
    egui::Panel::left("layers")
        .exact_size(106.0)
        .frame(margin)
        .show_separator_line(false)
        .show(ui, |ui| layers_sidebar(app, ui));
    if app.palette_open {
        // The keyboard comes first: on a small window the palette gives way.
        let palette_h = (ui.available_height() * PALETTE_SHARE).clamp(PALETTE_MIN_H, PALETTE_MAX_H);
        egui::Panel::bottom("palette")
            .exact_size(palette_h)
            .frame(Frame::new().inner_margin(egui::Margin { left: 0, right: 8, top: 4, bottom: 8 }))
            .show_separator_line(false)
            .show(ui, |ui| palette(app, ui));
    }
    egui::Panel::bottom("info")
        .exact_size(INFO_H)
        .frame(Frame::new().inner_margin(egui::Margin { left: 0, right: 8, top: 6, bottom: 2 }))
        .show_separator_line(false)
        .show(ui, |ui| info_bar(app, ui));
    egui::Panel::top("kb_tools")
        .frame(Frame::new().inner_margin(egui::Margin { left: 0, right: 8, top: 8, bottom: 6 }))
        .show_separator_line(false)
        .show(ui, |ui| toolbar(app, ui));
    if app.check_open {
        egui::Panel::right("check")
            .exact_size(250.0)
            .frame(Frame::new().inner_margin(egui::Margin { left: 0, right: 8, top: 0, bottom: 0 }))
            .show_separator_line(false)
            .show(ui, |ui| check_panel(app, ui));
    }
    egui::CentralPanel::default()
        .frame(Frame::new().inner_margin(egui::Margin { left: 0, right: 8, top: 0, bottom: 0 }))
        .show(ui, |ui| keyboard(app, ui));
    shortcuts(app, ui);
    // A drag ends wherever the pointer is released.
    if ui.input(|i| i.pointer.any_released()) {
        app.drag = None;
    }
}

fn layers_sidebar(app: &mut App, ui: &mut egui::Ui) {
    ui.vertical_centered_justified(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(RichText::new("Layers").size(11.0).color(theme::FG_MUTED));
        let footer = 30.0 + 30.0 + 4.0 + 8.0 + if app.online() { 28.0 } else { 0.0 };
        let list_h = (ui.available_height() - footer).max(40.0);
        egui::ScrollArea::vertical().max_height(list_h).auto_shrink([false, true]).show(ui, |ui| {
            let names = app.layer_names.clone();
            for (i, name) in names.iter().enumerate() {
                let active = i == app.layer;
                let empty = app.keys.iter().all(|k| app.doc.get((i, k.row, k.col)).unwrap_or(0) == 0);
                let fill = if active {
                    theme::PURPLE
                } else if empty {
                    theme::BG_PRIMARY.gamma_multiply(0.5)
                } else {
                    theme::BUTTON_BG
                };
                let text = if active {
                    theme::BG_PRIMARY
                } else if empty {
                    theme::FG_MUTED
                } else {
                    theme::FG_PRIMARY
                };
                let b = egui::Button::new(RichText::new(name).size(11.0).color(text))
                    .fill(fill)
                    .min_size(vec2(ui.available_width(), 30.0));
                if ui.add(b).clicked() {
                    app.switch_layer(i);
                }
            }
        });
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let w = ui.available_width();
            let current = app.kb_layout;
            egui::ComboBox::from_id_salt("os_layout")
                .width(w)
                .selected_text(RichText::new(current.name()).size(11.0))
                .show_ui(ui, |ui| {
                    for l in KeyboardLayout::all() {
                        if ui.selectable_label(*l == current, l.name()).clicked() {
                            app.set_kb_layout(*l);
                        }
                    }
                });
            let fill = if app.heatmap_on { theme::ORANGE } else { theme::BUTTON_BG };
            let heat = egui::Button::new(RichText::new("Heatmap").size(11.0))
                .fill(fill)
                .min_size(vec2(w, 30.0));
            if ui.add(heat).clicked() {
                app.heatmap_on = !app.heatmap_on;
                if app.heatmap_on && app.online() {
                    app.be.load_heatmap();
                }
            }
            if app.online() {
                let b = egui::Button::new(RichText::new("Rename").size(10.0)).min_size(vec2(w, 24.0));
                if ui.add(b).clicked() {
                    app.rename = Some(app.layer_names.get(app.layer).cloned().unwrap_or_default());
                }
            }
        });
    });
}

fn toolbar(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        ui.spacing_mut().button_padding = vec2(6.0, 5.0);
        if ui.add_enabled(app.doc.can_undo(), egui::Button::new("↶ Undo")).on_hover_text("Ctrl+Z").clicked() {
            app.undo();
        }
        if ui.add_enabled(app.doc.can_redo(), egui::Button::new("↷ Redo")).on_hover_text("Ctrl+Y").clicked() {
            app.redo();
        }
        let has_sel = app.selected.is_some();
        if ui.add_enabled(has_sel, egui::Button::new("Copy")).on_hover_text("Ctrl+C").clicked() {
            copy_selected(app);
        }
        let can_paste = has_sel && app.doc.clipboard.is_some();
        if ui.add_enabled(can_paste, egui::Button::new("Paste")).on_hover_text("Ctrl+V").clicked() {
            paste_selected(app);
        }
        ui.separator();
        let cap_fill = if app.capture { theme::GREEN } else { theme::BUTTON_BG };
        let cap_text = if app.capture { theme::BG_PRIMARY } else { theme::FG_PRIMARY };
        let cap = egui::Button::new(RichText::new("⌨ Type to assign").color(cap_text)).fill(cap_fill);
        if ui
            .add_enabled(has_sel, cap)
            .on_hover_text("Each key you press is assigned to the selected key, then the selection moves on. Esc stops.")
            .clicked()
        {
            app.capture = !app.capture;
        }
        ui.separator();
        let has_factory = app.factory_layer().is_some();
        ui.menu_button("Layer ▾", |ui| {
            if ui.button("Copy this layer").clicked() {
                app.copy_layer();
                ui.close();
            }
            if ui.add_enabled(app.doc.layer_clipboard.is_some(), egui::Button::new("Paste onto this layer")).clicked() {
                app.paste_layer();
                ui.close();
            }
            let factory = egui::Button::new("Back to factory keys");
            if ui
                .add_enabled(has_factory, factory)
                .on_hover_text("Put back this layer's keys from the firmware's default keymap")
                .clicked()
            {
                let model = app.boards.get(app.board).map(|b| b.name.clone()).unwrap_or_default();
                if app.online() {
                    app.confirm = Some(Confirm {
                        title: format!("Restore layer {} on the keyboard?", app.layer),
                        body: format!("Writes the {model} factory keys of this layer to the keyboard. Undo puts yours back."),
                        ok: "Restore".into(),
                        action: Action::RestoreFactoryLayer,
                    });
                } else {
                    app.restore_factory_layer();
                }
                ui.close();
            }
        });
        let findings = app.findings();
        let warnings = findings.iter().filter(|f| f.level == Level::Warning).count();
        let label = if warnings > 0 { format!("Check ({warnings})") } else { "Check".to_string() };
        let text = if warnings > 0 { theme::BG_PRIMARY } else { theme::FG_PRIMARY };
        let fill = if warnings > 0 { theme::ORANGE } else { theme::BUTTON_BG };
        if ui
            .add(egui::Button::new(RichText::new(label).color(text)).fill(fill))
            .on_hover_text("Look over the whole keymap for keys that won't do what you expect")
            .clicked()
        {
            app.check_open = !app.check_open;
        }
        ui.separator();
        ui.checkbox(&mut app.tint, "Colors");
        ui.add(egui::TextEdit::singleline(&mut app.find).hint_text("Find a key").desired_width(72.0));
    });
    if app.doc.pending_count() > 0 {
        ui.add_space(4.0);
        Frame::new()
            .fill(theme::ORANGE.gamma_multiply(0.25))
            .corner_radius(4)
            .inner_margin(egui::Margin::symmetric(8, 3))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let n = app.doc.pending_count();
                    if app.online() {
                        ui.label(format!("{n} key(s) not on the keyboard yet"));
                        let total = n;
                        if ui.button("Send to keyboard").clicked() {
                            app.confirm = Some(Confirm {
                                title: "Send these keys?".into(),
                                body: format!(
                                    "Writes {total} key(s) edited without a keyboard to the connected keyboard."
                                ),
                                ok: "Send".into(),
                                action: Action::SendPending,
                            });
                        }
                    } else {
                        ui.label(format!("{n} key(s) edited without a keyboard: they wait here until you connect one"));
                    }
                    if ui.button("Discard").clicked() {
                        app.doc.discard_pending();
                    }
                });
            });
    }
}

fn check_panel(app: &mut App, ui: &mut egui::Ui) {
    let findings = app.findings();
    Frame::new().fill(theme::BG_SURFACE).corner_radius(8).inner_margin(8).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(ui.available_height());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Keymap check").size(13.0).color(theme::CYAN));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("X").clicked() {
                    app.check_open = false;
                }
            });
        });
        if findings.is_empty() {
            ui.label(RichText::new("✓ Nothing to report.").color(theme::GREEN));
        }
        if !app.macros_loaded || !app.tap_dances_loaded {
            ui.label(
                RichText::new("Macros and tap dances aren't read yet: keys using them aren't checked.")
                    .size(11.0)
                    .color(theme::FG_MUTED),
            );
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for f in &findings {
                let (mark, color) = match f.level {
                    Level::Warning => ("⚠", theme::ORANGE),
                    Level::Note => ("•", theme::FG_MUTED),
                };
                let line = ui.add(
                    egui::Label::new(
                        RichText::new(format!("{mark} {}", f.text)).size(11.0).color(if f.level == Level::Warning {
                            theme::FG_PRIMARY
                        } else {
                            color
                        }),
                    )
                    .wrap()
                    .sense(Sense::click()),
                );
                if f.clearable {
                    if let Some(pos) = f.at {
                        let clear = ui.small_button("Clear this key").on_hover_text("Undo puts it back");
                        if clear.clicked() {
                            app.apply_edits(&[(pos, 0)]);
                            app.set_status(format!("Cleared R{}C{}", pos.1, pos.2));
                        }
                    }
                }
                if line.clicked() {
                    match (f.at, f.layer) {
                        (Some((l, r, c)), _) => app.jump_to(l, Some((r, c))),
                        (None, Some(l)) => app.jump_to(l, None),
                        _ => {}
                    }
                }
                ui.add_space(4.0);
            }
        });
    });
}

fn copy_selected(app: &mut App) {
    if let Some(pos) = app.selected.and_then(|i| app.pos_of(i)) {
        app.doc.clipboard = app.doc.get(pos);
        app.set_status(match app.doc.clipboard {
            Some(c) => format!("Copied 0x{c:04X}"),
            None => "Nothing to copy: this key isn't known yet".to_string(),
        });
    }
}

fn paste_selected(app: &mut App) {
    if let Some(code) = app.doc.clipboard {
        app.assign_selected(code);
    }
}

fn key_label(app: &App, idx: usize) -> (String, bool) {
    let k = &app.keys[idx];
    let pos = (app.layer, k.row, k.col);
    match app.doc.get(pos) {
        None => (format!("{},{}", k.col, k.row), false),
        Some(0) => match app.doc.fall_through(pos) {
            Some(below) => (keys::label_for(below, &app.kb_layout), true),
            None => (String::new(), true),
        },
        Some(code) => (keys::label_for(code, &app.kb_layout), false),
    }
}

fn presses(app: &App, row: usize, col: usize) -> Option<u32> {
    app.stats.heat.get(row)?.get(col).copied()
}

fn keyboard(app: &mut App, ui: &mut egui::Ui) {
    let (area, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    ui.painter().rect_filled(area, 8.0, theme::BG_SURFACE);
    let painter = ui.painter_at(area);
    let view = View::fit(&app.keys, area);
    app.kb_view = Some(view);
    let pointer = ui.input(|i| i.pointer.latest_pos()).filter(|p| area.contains(*p));
    let hovered = pointer.and_then(|p| shape::key_at(&view, &app.keys, p));
    let find = app.find.to_lowercase();

    for i in 0..app.keys.len() {
        let k = app.keys[i].clone();
        let pos = (app.layer, k.row, k.col);
        let (label, dim) = key_label(app, i);
        let code = app.doc.get(pos);
        let heat = app.heatmap_on.then(|| {
            let count = presses(app, k.row, k.col).unwrap_or(0);
            if app.stats.heat_max > 0 { count as f32 / app.stats.heat_max as f32 } else { 0.0 }
        });
        let heated = heat.filter(|&h| h > 0.0);
        let base = if let Some(h) = heated {
            theme::heat_color(h)
        } else if app.tint {
            code.map(|c| family_fill(keys::family(c))).unwrap_or(theme::KEYCAP)
        } else {
            theme::KEYCAP
        };
        let selected = app.selected == Some(i);
        let mut style = CapStyle {
            fill: if selected { theme::PURPLE } else { base },
            stroke: Stroke::new(1.0, if selected { theme::CYAN } else { theme::BG_PRIMARY }),
            text: if selected {
                theme::BG_PRIMARY
            } else if heated.is_some_and(|h| h > 0.2) {
                theme::DARK_TEXT
            } else if dim {
                theme::FG_SECONDARY
            } else {
                theme::FG_PRIMARY
            },
        };
        if app.doc.is_pending(pos) {
            style.stroke = Stroke::new(2.0, theme::ORANGE);
        }
        let dragging_over = app.drag.is_some() && hovered == Some(i);
        if !find.is_empty() && !dim && label.to_lowercase().contains(&find) {
            style.stroke = Stroke::new(2.0, theme::YELLOW);
        }
        if dragging_over {
            style.stroke = Stroke::new(2.0, theme::GREEN);
        } else if hovered == Some(i) && !selected {
            style.fill = style.fill.gamma_multiply(1.25);
        }
        paint_key(&painter, &view, &k, &style, &label);
    }

    if resp.drag_started() {
        let origin = ui.input(|i| i.pointer.press_origin());
        if let Some(i) = origin.and_then(|p| shape::key_at(&view, &app.keys, p)) {
            app.drag = Some(DragItem::Key(i));
            app.selected = Some(i);
        }
    }
    if ui.input(|i| i.pointer.any_released()) {
        if let (Some(item), Some(target)) = (app.drag.take(), hovered) {
            let copying = ui.input(|i| i.modifiers.ctrl);
            match item {
                DragItem::Code(code) => {
                    app.selected = Some(target);
                    app.assign_key(target, code);
                }
                DragItem::Key(from) if from != target => {
                    app.selected = Some(target);
                    if copying {
                        if let Some(code) = app.pos_of(from).and_then(|p| app.doc.get(p)) {
                            app.assign_key(target, code);
                        }
                    } else {
                        app.swap_keys(from, target);
                    }
                }
                DragItem::Key(_) => {}
            }
        }
    }
    if resp.double_clicked() {
        if hovered.is_some() {
            app.selected = hovered;
            app.capture = false;
            app.open_selector(Target::Keymap);
        }
    } else if resp.clicked() {
        app.selected = hovered;
        if hovered.is_none() {
            app.capture = false;
        }
    }
    if resp.secondary_clicked() && hovered.is_some() {
        app.selected = hovered;
    }
    resp.context_menu(|ui| {
        if ui.button("Change key...").clicked() {
            app.open_selector(Target::Keymap);
            ui.close();
        }
        if ui.button("Copy").clicked() {
            copy_selected(app);
            ui.close();
        }
        if ui.add_enabled(app.doc.clipboard.is_some(), egui::Button::new("Paste")).clicked() {
            paste_selected(app);
            ui.close();
        }
        if ui.button("Clear").clicked() {
            app.assign_selected(0);
            ui.close();
        }
    });

    if let (Some(i), None) = (hovered, app.drag.as_ref()) {
        let k = &app.keys[i];
        let code = app.doc.get((app.layer, k.row, k.col));
        let mut tip = format!("R{}C{}", k.row, k.col);
        if let Some(c) = code {
            tip += &format!("\n{}\n0x{c:04X}", crate::protocol::keycode::decode_keycode(c));
        }
        if let Some(n) = presses(app, k.row, k.col) {
            tip += &format!("\n{n} presses");
        }
        resp.clone().on_hover_text_at_pointer(tip);
    }

    if let (Some(item), Some(p)) = (&app.drag, ui.input(|i| i.pointer.latest_pos())) {
        let label = match item {
            DragItem::Code(c) => keys::label_for(*c, &app.kb_layout),
            DragItem::Key(i) => key_label(app, *i).0,
        };
        let top = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("drag_ghost")));
        let rect = egui::Rect::from_center_size(p, vec2(52.0, 34.0));
        top.rect_filled(rect, 4.0, theme::PURPLE.gamma_multiply(0.9));
        top.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(12.0),
            theme::BG_PRIMARY,
        );
    }
}

fn info_bar(app: &mut App, ui: &mut egui::Ui) {
    Frame::new()
        .fill(theme::BG_PRIMARY.gamma_multiply(0.45))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                match app.selected.and_then(|i| app.keys.get(i).map(|k| (k.row, k.col))) {
                    Some((row, col)) => {
                        let code = app.doc.get((app.layer, row, col));
                        let name = code
                            .map(|c| keys::label_for(c, &app.kb_layout))
                            .unwrap_or_else(|| "?".into());
                        ui.label(RichText::new(format!("Selected: {name}")).color(theme::CYAN));
                        let mut extra = format!("R{row}C{col}");
                        if let Some(c) = code {
                            extra += &format!("   0x{c:04X}");
                        }
                        ui.label(RichText::new(extra).color(theme::FG_MUTED).size(11.0));
                        if app.capture {
                            ui.label(RichText::new("Press a key... (Esc stops)").color(theme::GREEN));
                        }
                    }
                    None => {
                        ui.label(
                            RichText::new("Click to select, double-click to change, drag to swap")
                                .color(theme::FG_MUTED)
                                .size(11.0),
                        );
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if app.palette_open { "Hide keys ▾" } else { "Show keys ▴" };
                    if ui.button(label).clicked() {
                        app.palette_open = !app.palette_open;
                    }
                    if ui.add_enabled(app.selected.is_some(), egui::Button::new("Change Key...")).clicked() {
                        app.open_selector(Target::Keymap);
                    }
                    if app.tint {
                        for (name, family) in LEGEND.iter().rev() {
                            let (r, swatch) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                            ui.painter().rect_filled(r, 2.0, family_fill(family));
                            swatch.on_hover_text(*name);
                        }
                    }
                });
            });
        });
}

fn palette(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
        ui.spacing_mut().button_padding = vec2(6.0, 3.0);
        for (i, (name, _)) in SECTIONS.iter().enumerate() {
            let short = if *name == "Tap Dance / Macros" { "TD / Macros" } else { name.split(" / ").next().unwrap_or(name) };
            if ui.selectable_label(app.palette_section == i, RichText::new(short).size(11.0)).clicked() {
                app.palette_section = i;
            }
        }
    });
    ui.add_space(3.0);
    Frame::new()
        .fill(theme::BG_PRIMARY)
        .corner_radius(4)
        .inner_margin(6)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
                    let (_, cats) = SECTIONS[app.palette_section];
                    let tiles: Vec<(String, u16)> = keys::section_entries(&app.catalog, cats, "")
                        .into_iter()
                        .map(|e| (e.name.clone(), e.code))
                        .collect();
                    for (name, code) in tiles {
                        let b = egui::Button::new(RichText::new(&name).size(11.0))
                            .min_size(vec2(44.0, 26.0))
                            .sense(Sense::click_and_drag());
                        let r = ui.add(b).on_hover_text(format!("0x{code:04X}: drag onto a key, or click to set the selected key"));
                        if r.drag_started() {
                            app.drag = Some(DragItem::Code(code));
                        }
                        if r.clicked() {
                            app.assign_selected(code);
                        }
                    }
                });
            });
        });
}

/// Keyboard use of the keyboard view. Stays out of the way of text fields and dialogs.
fn shortcuts(app: &mut App, ui: &mut egui::Ui) {
    if app.any_dialog_open() || ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    if app.capture {
        capture_keys(app, ui);
        return;
    }
    let (undo, redo, copy, paste, clear, enter) = ui.input_mut(|i| {
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        (
            i.consume_key(Modifiers::COMMAND, Key::Z),
            i.consume_key(Modifiers::COMMAND, Key::Y) || i.consume_key(cmd_shift, Key::Z),
            i.consume_key(Modifiers::COMMAND, Key::C),
            i.consume_key(Modifiers::COMMAND, Key::V),
            i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
            i.key_pressed(Key::Enter),
        )
    });
    if undo {
        app.undo();
    }
    if redo {
        app.redo();
    }
    if copy {
        copy_selected(app);
    }
    if paste {
        paste_selected(app);
    }
    if let Some(sel) = app.selected {
        if clear {
            app.assign_key(sel, 0);
        }
        if enter {
            app.open_selector(Target::Keymap);
        }
        for (key, dir) in [
            (Key::ArrowLeft, vec2(-1.0, 0.0)),
            (Key::ArrowRight, vec2(1.0, 0.0)),
            (Key::ArrowUp, vec2(0.0, -1.0)),
            (Key::ArrowDown, vec2(0.0, 1.0)),
        ] {
            if ui.input(|i| i.key_pressed(key)) {
                if let Some(next) = shape::neighbour(&app.keys, sel, dir) {
                    app.selected = Some(next);
                }
            }
        }
    }
}

fn capture_keys(app: &mut App, ui: &mut egui::Ui) {
    let pressed: Vec<egui::Key> = ui.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { key, physical_key, pressed: true, repeat: false, .. } => {
                    Some(physical_key.unwrap_or(*key))
                }
                _ => None,
            })
            .collect()
    });
    for key in pressed {
        if key == Key::Escape {
            app.capture = false;
            app.set_status("Stopped assigning");
            return;
        }
        let Some(code) = keys::hid_from_egui(key) else { continue };
        let Some(sel) = app.selected else { return };
        app.assign_key(sel, code);
        // Next key of the row, so a whole row can be typed in one go.
        app.selected = shape::neighbour(&app.keys, sel, vec2(1.0, 0.0)).or(Some(sel));
    }
}
