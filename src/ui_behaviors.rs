//! The Behaviors tab: everything a key can trigger beyond its keycode, one kind at
//! a time. Macros, tap dances, combos, overrides, leader sequences, layer rules.

use crate::app::{App, Behavior, Target};
use crate::backend::Refresh;
use crate::checks;
use crate::keys;
use crate::protocol::binary::{self as bp, cmd};
use crate::protocol::keycode;
use crate::shape::{self, View};
use crate::theme;
use crate::ui_common::{card, note, page_head, paint_key, primary, side_nav, CapStyle};
use egui::{vec2, Frame, RichText, Sense, Stroke};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let td_used = app.adv.tap_dances.iter().filter(|t| t.iter().any(|&a| a != 0)).count();
    let items = [
        (Behavior::Macros, "Macros", app.macros.len()),
        (Behavior::TapDance, "Tap Dance", td_used),
        (Behavior::Combos, "Combos", app.adv.combos.len()),
        (Behavior::Overrides, "Key Overrides", app.adv.key_overrides.len()),
        (Behavior::Leader, "Leader Keys", app.adv.leaders.len()),
        (Behavior::Layers, "Layers & Shift", 0),
    ];
    let mut current = app.behavior;
    side_nav(ui, "behaviors_nav", &items, &mut current);
    app.behavior = current;
    egui::CentralPanel::default().frame(Frame::new()).show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            Frame::new().inner_margin(16).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 8.0;
                match app.behavior {
                    Behavior::Macros => macros(app, ui),
                    Behavior::TapDance => tap_dance(app, ui),
                    Behavior::Combos => combos(app, ui),
                    Behavior::Overrides => overrides(app, ui),
                    Behavior::Leader => leader(app, ui),
                    Behavior::Layers => layers(app, ui),
                }
            });
        });
    });
}

fn refresh_button(app: &mut App, ui: &mut egui::Ui, title: &str, about: &str) {
    if page_head(ui, title, about, "Refresh", app.online()) {
        app.be.refresh_advanced();
        app.be.refresh_macros();
    }
}

fn name_or_pick(code: Option<u16>) -> String {
    code.map(keycode::decode_keycode).unwrap_or_else(|| "Pick...".to_string())
}

fn pick_button(app: &mut App, ui: &mut egui::Ui, label: &str, target: Target) {
    let b = egui::Button::new(RichText::new(label).size(11.0)).min_size(vec2(100.0, 28.0));
    if ui.add(b).clicked() {
        app.open_selector(target);
    }
}

/// "on 2 keys", with where in the tooltip; clicking it goes to the first one.
fn used_on_label(app: &mut App, ui: &mut egui::Ui, all: &[checks::Key], code: u16) {
    let places = checks::used_on(all, code);
    if places.is_empty() {
        let link = ui
            .add(egui::Label::new(RichText::new("on no key").size(10.0).color(theme::CYAN)).sense(Sense::click()))
            .on_hover_text("Go to the Keymap tab, to drag it onto a key");
        if link.clicked() {
            app.tab = crate::app::Tab::Keymap;
            app.palette_open = true;
            app.palette_section = keys::SECTIONS.len() - 1;
            app.set_status("Drag it from the palette onto a key");
        }
        return;
    }
    let text = if places.len() == 1 { "on 1 key".to_string() } else { format!("on {} keys", places.len()) };
    let list: Vec<String> = places
        .iter()
        .map(|&(l, r, c)| format!("{} R{r}C{c}", app.layer_names.get(l).cloned().unwrap_or_default()))
        .collect();
    let link = ui
        .add(egui::Label::new(RichText::new(text).size(10.0).color(theme::CYAN)).sense(Sense::click()))
        .on_hover_text(list.join("\n"));
    if link.clicked() {
        let (l, r, c) = places[0];
        app.jump_to(l, Some((r, c)));
    }
}

/// One line of a list: id, what triggers it, what it does, and a delete button.
fn item_row(ui: &mut egui::Ui, online: bool, prefix: &str, left: &str, right: &str) -> (bool, bool) {
    let mut del = false;
    let inner = Frame::new()
        .fill(theme::BG_SECONDARY)
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(prefix).size(11.0).color(theme::FG_MUTED));
                ui.label(RichText::new(left).size(12.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    del = ui.add_enabled(online, egui::Button::new("Del")).clicked();
                    ui.label(RichText::new(right).size(12.0));
                    ui.label(RichText::new("→").color(theme::FG_MUTED));
                });
            });
        });
    (del, inner.response.hovered() || inner.response.contains_pointer())
}

/// A key was picked and it isn't "None".
fn has_key(code: Option<u16>) -> bool {
    code.is_some_and(|c| c != 0)
}

const fn mod_bits(m: [bool; 3]) -> u8 {
    (m[0] as u8) | ((m[1] as u8) << 1) | ((m[2] as u8) << 2)
}

fn mods_with(mods: u8, key: u8) -> String {
    if mods != 0 {
        format!("{}+{}", keycode::mod_name(mods), keycode::hid_key_name(key))
    } else {
        keycode::hid_key_name(key)
    }
}

fn macros(app: &mut App, ui: &mut egui::Ui) {
    refresh_button(app, ui, "Macros", "A key that plays a sequence: other keys, shortcuts and pauses.");
    if app.macros.is_empty() {
        note(ui, "No macros on this keyboard yet.");
    }
    let online = app.online();
    let mut delete = None;
    let all = app.all_keys();
    let mut used = Vec::new();
    for m in &app.macros {
        let together: Vec<String> = m.keys.iter().map(|&k| keycode::hid_key_name(k)).collect();
        let mut steps: Vec<String> = m
            .steps
            .iter()
            .map(|s| if s.is_delay() { format!("T({})", s.delay_ms()) } else { keycode::hid_key_name(s.keycode) })
            .collect();
        if !together.is_empty() {
            steps.insert(0, together.join("+"));
        }
        if item_row(ui, online, &format!("M{}", m.slot), &m.name, &steps.join(" ")).0 {
            delete = Some(m.slot);
        }
        used.push(m.slot);
    }
    if !used.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for slot in used {
                ui.label(RichText::new(format!("M{slot}")).size(10.0).color(theme::FG_MUTED));
                used_on_label(app, ui, &all, 0x1500 + ((slot as u16) << 8));
                ui.add_space(8.0);
            }
        });
    }
    if let Some(slot) = delete {
        app.be.command_then(cmd::MACRO_DELETE, bp::macro_delete_payload(slot), Refresh::Macros);
    }
    ui.add_space(6.0);
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("New macro, slot M{}", app.mac.slot)).size(13.0).color(theme::CYAN));
            ui.add(egui::TextEdit::singleline(&mut app.mac.name).hint_text("macro name").desired_width(f32::INFINITY));
            app.mac.name = keys::fit_macro_name(&app.mac.name);
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            if app.mac.steps.is_empty() {
                note(ui, "No steps yet: add keys and pauses below.");
            }
            for &(kc, md) in &app.mac.steps {
                let (label, fill) = if kc == 0xFF {
                    (format!("wait {} ms", md as u32 * 10), theme::ORANGE)
                } else {
                    (keycode::hid_key_name(kc), theme::PURPLE)
                };
                Frame::new()
                    .fill(fill)
                    .corner_radius(4)
                    .inner_margin(egui::Margin::symmetric(6, 5))
                    .show(ui, |ui| ui.label(RichText::new(label).size(11.0).color(theme::BG_PRIMARY)));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if ui.button("Add Key").clicked() {
                app.open_selector(Target::MacroStep);
            }
            for ms in [50u32, 100, 200, 500] {
                if ui.button(format!("{ms} ms")).clicked() {
                    app.mac.steps.push((0xFF, (ms / 10) as u8));
                }
            }
            if ui.add_enabled(!app.mac.steps.is_empty(), egui::Button::new("Undo")).clicked() {
                app.mac.steps.pop();
            }
            if ui.add_enabled(!app.mac.steps.is_empty(), egui::Button::new("Clear")).clicked() {
                app.mac.steps.clear();
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.spacing_mut().button_padding = vec2(8.0, 4.0);
            note(ui, "Shortcuts:");
            for (name, modifier, key) in [
                ("Ctrl+C", 0xE0u8, 0x06u8),
                ("Ctrl+V", 0xE0, 0x19),
                ("Ctrl+X", 0xE0, 0x1B),
                ("Ctrl+Z", 0xE0, 0x1D),
                ("Ctrl+Y", 0xE0, 0x1C),
                ("Ctrl+S", 0xE0, 0x16),
                ("Ctrl+A", 0xE0, 0x04),
                ("Alt+F4", 0xE2, 0x3D),
            ] {
                if ui.button(name).clicked() {
                    app.mac.steps.extend([(modifier, 0), (key, 0)]);
                }
            }
        });
        // The firmware holds every key of a macro with no pause together, as one
        // shortcut (key_processor.c, expand_macro): fine for Ctrl+C, not for a word.
        let keys_only = app.mac.steps.iter().filter(|s| s.0 != 0xFF).count();
        if keys_only > 1 && app.mac.steps.iter().all(|s| s.0 != 0xFF) {
            ui.label(
                RichText::new("With no pause, these keys are pressed together, as one shortcut. Add a pause to type them one after another.")
                    .size(11.0)
                    .color(theme::ORANGE),
            );
        }
        let too_long = app.mac.steps.len() > keys::MACRO_STEPS;
        if too_long {
            ui.label(
                RichText::new(format!("{} steps: the keyboard keeps {}.", app.mac.steps.len(), keys::MACRO_STEPS))
                    .size(11.0)
                    .color(theme::ORANGE),
            );
        }
        let room = (app.mac.slot as u16) < keys::MACRO_KEYS;
        if !room {
            ui.label(RichText::new("All macro slots are used: delete one first.").size(11.0).color(theme::ORANGE));
        }
        let ready = app.online() && !app.mac.steps.is_empty() && !app.mac.name.trim().is_empty() && !too_long && room;
        if ui.add_enabled(ready, primary("Save Macro")).clicked() {
            let slot = app.mac.slot as u8;
            let steps: Vec<String> = app.mac.steps.iter().map(|(k, m)| format!("{k:02X}:{m:02X}")).collect();
            let payload = bp::macro_add_seq_payload(slot, &app.mac.name, &steps.join(","));
            app.be.command_then(cmd::MACRO_ADD_SEQ, payload, Refresh::Macros);
            app.status = format!("Saving macro #{slot}...");
            app.mac.name.clear();
            app.mac.steps.clear();
        }
    });
}

fn tap_dance(app: &mut App, ui: &mut egui::Ui) {
    refresh_button(app, ui, "Tap Dance", "One key, four actions: tap once, twice, three times, or hold. Put a TD key on the Keymap tab to use one.");
    card(ui, |ui| {
        egui::Grid::new("td").num_columns(6).spacing([6.0, 4.0]).show(ui, |ui| {
            ui.label("");
            for head in ["1 tap", "2 taps", "3 taps", "Hold", ""] {
                ui.label(RichText::new(head).size(11.0).color(theme::FG_MUTED));
            }
            ui.end_row();
            let all = app.all_keys();
            for i in 0..app.adv.tap_dances.len() {
                ui.label(RichText::new(format!("TD {i}")).size(11.0).color(theme::FG_MUTED));
                for slot in 0..4 {
                    let code = app.adv.tap_dances[i][slot];
                    let name = if code == 0 { "—".to_string() } else { keycode::decode_keycode(code) };
                    let b = egui::Button::new(RichText::new(name).size(11.0)).min_size(vec2(84.0, 26.0));
                    if ui.add_enabled(app.online(), b).clicked() {
                        app.open_selector(Target::TdAction(i, slot));
                    }
                }
                used_on_label(app, ui, &all, 0x6000 | ((i as u16) << 8));
                ui.end_row();
            }
        });
    });
}

/// Keyboard to click for a combo's two keys. Keys of the combo under the pointer
/// light up, and so do the two being chosen.
fn combo_keyboard(app: &mut App, ui: &mut egui::Ui, hovered: Option<[(usize, usize); 2]>) {
    let height = (app.keys.len() as f32 * 4.5).clamp(170.0, 300.0);
    let (area, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    ui.painter().rect_filled(area, 8.0, theme::BG_SURFACE);
    let view = View::fit(&app.keys, area);
    app.combo_view = Some(view);
    let hover = resp.hover_pos().and_then(|p| shape::key_at(&view, &app.keys, p));
    let painter = ui.painter_at(area);
    for (i, k) in app.keys.iter().enumerate() {
        let at = (k.row, k.col);
        let chosen = app.adv.combo_keys.contains(&Some(at));
        let lit = hovered.is_some_and(|h| h.contains(&at));
        let code = app.doc.get((0, k.row, k.col));
        let label = code.map(|c| keys::label_for(c, &app.kb_layout)).unwrap_or_else(|| format!("R{}C{}", k.row, k.col));
        let (fill, text) = if chosen {
            (theme::PURPLE, theme::BG_PRIMARY)
        } else if lit {
            (theme::CYAN, theme::BG_PRIMARY)
        } else if hover == Some(i) {
            (theme::BUTTON_HOVER, theme::FG_PRIMARY)
        } else {
            (theme::KEYCAP, theme::FG_PRIMARY)
        };
        let style = CapStyle { fill, stroke: Stroke::new(1.0, theme::BG_PRIMARY), text };
        paint_key(&painter, &view, k, &style, &label);
    }
    if resp.clicked() {
        if let Some(i) = hover {
            let at = (app.keys[i].row, app.keys[i].col);
            app.adv.combo_keys = next_combo_keys(app.adv.combo_keys, at);
        }
    }
}

/// A click adds a key, a click on a chosen key removes it, a third key replaces the second.
fn next_combo_keys(keys: [Option<(usize, usize)>; 2], at: (usize, usize)) -> [Option<(usize, usize)>; 2] {
    match keys {
        [Some(a), b] if a == at => [b, None],
        [a, Some(b)] if b == at => [a, None],
        [None, _] => [Some(at), None],
        [a, _] => [a, Some(at)],
    }
}

fn combos(app: &mut App, ui: &mut egui::Ui) {
    refresh_button(app, ui, "Combos", "Press two keys together to send a third. Click two keys, pick what they send.");
    let mut hovered = None;
    let mut delete = None;
    let online = app.online();
    let mut rows = Vec::new();
    for c in &app.adv.combos {
        rows.push((c.index, [(c.r1 as usize, c.c1 as usize), (c.r2 as usize, c.c2 as usize)], keycode::decode_keycode(c.result)));
    }
    // Drawn after the list is measured, so a hovered combo lights up the same frame.
    let list_id = ui.make_persistent_id("combo_hover");
    let last_hover: Option<[(usize, usize); 2]> = ui.data(|d| d.get_temp(list_id));
    combo_keyboard(app, ui, last_hover);
    ui.horizontal_wrapped(|ui| {
        let name = |k: Option<(usize, usize)>| k.map(|(r, c)| format!("R{r}C{c}")).unwrap_or_else(|| "click a key".to_string());
        ui.label(format!("{} + {}", name(app.adv.combo_keys[0]), name(app.adv.combo_keys[1])));
        ui.label(RichText::new("→").color(theme::FG_MUTED));
        let n = name_or_pick(app.adv.combo_result);
        pick_button(app, ui, &n, Target::ComboResult);
        // The firmware drops a combo or a leader sequence that sends nothing.
        let room = app.adv.combos.len() < keys::LIST_SLOTS as usize;
        let ready = online && app.adv.combo_keys.iter().all(Option::is_some) && has_key(app.adv.combo_result) && room;
        if ui.add_enabled(ready, primary("Add Combo")).clicked() {
            if let ([Some(a), Some(b)], Some(result)) = (app.adv.combo_keys, app.adv.combo_result) {
                let index = keys::free_slot(app.adv.combos.iter().map(|c| c.index));
                let payload = bp::combo_set_payload(index, a.0 as u8, a.1 as u8, b.0 as u8, b.1 as u8, result as u8);
                app.be.command_then(cmd::COMBO_SET, payload, Refresh::Combos);
                app.adv.combo_keys = [None, None];
                app.set_status("Creating combo...");
            }
        }
    });
    if rows.is_empty() {
        note(ui, "No combos on this keyboard yet.");
    }
    for (index, at, result) in &rows {
        let left = format!("R{}C{} + R{}C{}", at[0].0, at[0].1, at[1].0, at[1].1);
        let (del, over) = item_row(ui, online, &format!("#{index}"), &left, result);
        if del {
            delete = Some(*index);
        }
        if over {
            hovered = Some(*at);
        }
    }
    ui.data_mut(|d| d.insert_temp(list_id, hovered));
    if let Some(i) = delete {
        app.be.command_then(cmd::COMBO_DELETE, vec![i], Refresh::Combos);
    }
}

fn overrides(app: &mut App, ui: &mut egui::Ui) {
    refresh_button(app, ui, "Key Overrides", "Replace a key, with optional modifiers, by another. Shift+1 gives ! by default: send something else.");
    if app.adv.key_overrides.is_empty() {
        note(ui, "No key overrides on this keyboard yet.");
    }
    let online = app.online();
    let mut delete = None;
    for ko in &app.adv.key_overrides {
        let (from, to) = (mods_with(ko.trigger_mod, ko.trigger_key), mods_with(ko.result_mod, ko.result_key));
        if item_row(ui, online, &format!("KO{}", ko.index), &from, &to).0 {
            delete = Some(ko.index);
        }
    }
    if let Some(i) = delete {
        app.be.command_then(cmd::KO_DELETE, vec![i], Refresh::KeyOverrides);
    }
    ui.add_space(6.0);
    card(ui, |ui| {
        ui.label(RichText::new("New override").size(13.0).color(theme::CYAN));
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("From").size(11.0).color(theme::FG_MUTED));
            let name = name_or_pick(app.adv.ko_trigger);
            pick_button(app, ui, &name, Target::KoTrigger);
            for (m, label) in app.adv.ko_trig_mods.iter_mut().zip(["Ctrl", "Shift", "Alt"]) {
                ui.checkbox(m, label);
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("To").size(11.0).color(theme::FG_MUTED));
            let name = name_or_pick(app.adv.ko_result);
            pick_button(app, ui, &name, Target::KoResult);
            for (m, label) in app.adv.ko_res_mods.iter_mut().zip(["Ctrl", "Shift", "Alt"]) {
                ui.checkbox(m, label);
            }
        });
        // The firmware lists an override only if it has a trigger.
        let slot = keys::free_slot(app.adv.key_overrides.iter().map(|k| k.index));
        let ready = online && has_key(app.adv.ko_trigger) && app.adv.ko_result.is_some() && slot < keys::LIST_SLOTS;
        if ui.add_enabled(ready, primary("Add Key Override")).clicked() {
            let payload = bp::ko_set_payload(
                slot,
                app.adv.ko_trigger.unwrap_or(0) as u8,
                mod_bits(app.adv.ko_trig_mods),
                app.adv.ko_result.unwrap_or(0) as u8,
                mod_bits(app.adv.ko_res_mods),
            );
            app.be.command_then(cmd::KO_SET, payload, Refresh::KeyOverrides);
            app.set_status("Creating key override...");
        }
    });
}

const MOD_CHOICES: [&str; 9] = ["None", "Ctrl", "Shift", "Alt", "GUI", "RCtrl", "RShift", "RAlt", "RGUI"];

fn leader(app: &mut App, ui: &mut egui::Ui) {
    refresh_button(app, ui, "Leader Keys", "Press the Leader key, then a sequence, to trigger an action. Vim-style shortcuts on your keyboard.");
    if app.adv.leaders.is_empty() {
        note(ui, "No leader sequences on this keyboard yet.");
    }
    let online = app.online();
    let mut delete = None;
    for l in &app.adv.leaders {
        let seq: Vec<String> = l.sequence.iter().map(|&k| keycode::hid_key_name(k)).collect();
        if item_row(ui, online, &format!("#{}", l.index), &seq.join(" → "), &mods_with(l.result_mod, l.result)).0 {
            delete = Some(l.index);
        }
    }
    if let Some(i) = delete {
        app.be.command_then(cmd::LEADER_DELETE, vec![i], Refresh::Leaders);
    }
    ui.add_space(6.0);
    card(ui, |ui| {
        ui.label(RichText::new("New sequence").size(13.0).color(theme::CYAN));
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Keys").size(11.0).color(theme::FG_MUTED));
            for &code in &app.adv.leader_seq {
                Frame::new()
                    .fill(theme::BUTTON_BG)
                    .corner_radius(3)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| ui.label(RichText::new(keycode::decode_keycode(code)).size(11.0)));
            }
            if app.adv.leader_seq.len() < 4 && ui.button("+").clicked() {
                app.open_selector(Target::LeaderSeq);
            }
            if !app.adv.leader_seq.is_empty() && ui.button("Clear").clicked() {
                app.adv.leader_seq.clear();
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Sends").size(11.0).color(theme::FG_MUTED));
            let n = name_or_pick(app.adv.leader_result);
            pick_button(app, ui, &n, Target::LeaderResult);
            ui.label(RichText::new("with").size(11.0).color(theme::FG_MUTED));
            egui::ComboBox::from_id_salt("leader_mod")
                .width(80.0)
                .show_index(ui, &mut app.adv.leader_mod, MOD_CHOICES.len(), |i| MOD_CHOICES[i]);
        });
        let room = app.adv.leaders.len() < keys::LIST_SLOTS as usize;
        let ready = online && !app.adv.leader_seq.is_empty() && has_key(app.adv.leader_result) && room;
        if ui.add_enabled(ready, primary("Add Leader Key")).clicked() {
            let seq: Vec<u8> = app.adv.leader_seq.iter().map(|&c| c as u8).collect();
            let result_mod = if app.adv.leader_mod == 0 { 0 } else { 1u8 << (app.adv.leader_mod - 1) };
            let index = keys::free_slot(app.adv.leaders.iter().map(|l| l.index));
            let payload = bp::leader_set_payload(index, &seq, app.adv.leader_result.unwrap_or(0) as u8, result_mod);
            app.be.command_then(cmd::LEADER_SET, payload, Refresh::Leaders);
            app.adv.leader_seq.clear();
            app.set_status("Creating leader key...");
        }
    });
}

fn layers(app: &mut App, ui: &mut egui::Ui) {
    refresh_button(app, ui, "Layers & Shift", "Rules that apply to the whole keyboard.");
    card(ui, |ui| {
        ui.label(RichText::new("Tri-Layer").size(13.0).color(theme::CYAN));
        note(ui, "When layers L1 and L2 are both active, activate L3. Handy with two thumb keys.");
        ui.horizontal_wrapped(|ui| {
            for (i, label) in ["L1", "+ L2", "→ L3"].into_iter().enumerate() {
                ui.label(RichText::new(label).color(theme::FG_MUTED));
                egui::ComboBox::from_id_salt(("tri", i))
                    .width(44.0)
                    .show_index(ui, &mut app.adv.tri[i], 10, |n| n.to_string());
            }
            if ui.add_enabled(app.online(), egui::Button::new("Set")).clicked() {
                let [a, b, c] = app.adv.tri;
                app.be.command(cmd::TRILAYER_SET, vec![a as u8, b as u8, c as u8]);
                app.status = format!("Tri-layer: {a} + {b} → {c}");
            }
        });
    });
    card(ui, |ui| {
        ui.label(RichText::new("Autoshift").size(13.0).color(theme::CYAN));
        note(ui, "Hold a key to send its shifted version: letters, numbers and symbols.");
        ui.horizontal(|ui| {
            let s = if app.adv.autoshift.is_empty() { "Unknown" } else { app.adv.autoshift.as_str() };
            ui.label(RichText::new(s).size(12.0));
            if ui.add_enabled(app.online(), egui::Button::new("Toggle")).clicked() {
                app.be.toggle_autoshift();
            }
        });
    });
    if !app.adv.bt_status.is_empty() {
        card(ui, |ui| {
            ui.label(RichText::new("Bluetooth").size(13.0).color(theme::CYAN));
            note(ui, "Up to 3 devices. Use the BT keys on your keymap to switch.");
            ui.label(RichText::new(&app.adv.bt_status).size(11.0));
        });
    }
    if !app.adv.tama_status.is_empty() {
        card(ui, |ui| {
            ui.label(RichText::new("Tamagotchi").size(13.0).color(theme::CYAN));
            ui.label(RichText::new(&app.adv.tama_status).size(11.0));
            ui.horizontal(|ui| {
                for (name, id) in [
                    ("Feed", cmd::TAMA_FEED),
                    ("Play", cmd::TAMA_PLAY),
                    ("Sleep", cmd::TAMA_SLEEP),
                    ("Meds", cmd::TAMA_MEDICINE),
                    ("On/Off", cmd::TAMA_ENABLE),
                ] {
                    if ui.add_enabled(app.online(), egui::Button::new(name)).clicked() {
                        app.be.tama_action(id);
                    }
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicking_keys_builds_a_combo_of_two_and_clicking_again_removes() {
        let (a, b, c) = ((0, 0), (1, 1), (2, 2));
        let k = next_combo_keys([None, None], a);
        assert_eq!(k, [Some(a), None]);
        let k = next_combo_keys(k, b);
        assert_eq!(k, [Some(a), Some(b)]);
        // A third key replaces the second.
        assert_eq!(next_combo_keys(k, c), [Some(a), Some(c)]);
        // Clicking a chosen key removes it, the other one stays first.
        assert_eq!(next_combo_keys(k, a), [Some(b), None]);
        assert_eq!(next_combo_keys(k, b), [Some(a), None]);
    }
}
