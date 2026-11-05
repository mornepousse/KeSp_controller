//! The key selector dialog: searchable tiles, Mod-Tap and Layer-Tap builders,
//! hex input, and a keyboard to click when a combo needs a key position.

use crate::app::{App, Target};
use crate::keys::{self, ALL_MODS, LT_KEYS, MOD_NAMES, MT_KEYS, SECTIONS};
use crate::protocol::keycode;
use crate::theme;
use egui::{vec2, Frame, RichText, Stroke};

pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(target) = app.selector else { return };
    let screen = ctx.content_rect();
    let size = vec2(480f32.min(screen.width() - 40.0), 520f32.min(screen.height() - 40.0));
    let modal = egui::Modal::new(egui::Id::unique("key_selector"))
        .frame(
            Frame::new()
                .fill(theme::BG_PRIMARY)
                .stroke(Stroke::new(1.0, theme::PURPLE))
                .corner_radius(12)
                .inner_margin(14),
        )
        .show(ctx, |ui| {
            ui.set_width(size.x - 28.0);
            ui.set_height(size.y - 28.0);
            let mut close = false;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Select Key").size(16.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.button("X").clicked();
                });
            });
            ui.add_space(4.0);
            let picked = code_picker(app, ui, target);
            close || picked
        });
    if modal.inner || modal.should_close() {
        app.selector = None;
    }
}

fn tile(ui: &mut egui::Ui, label: &str, code: u16, enabled: bool) -> bool {
    let b = egui::Button::new(RichText::new(label).size(11.0)).min_size(vec2(55.0, 30.0));
    ui.add_enabled(enabled, b).on_hover_text(format!("0x{code:04X}")).clicked()
}

fn code_picker(app: &mut App, ui: &mut egui::Ui, target: Target) -> bool {
    let only_basic = target.basic_keys_only();
    ui.add(
        egui::TextEdit::singleline(&mut app.search)
            .hint_text("Search...")
            .desired_width(f32::INFINITY),
    );
    if only_basic {
        ui.label(
            RichText::new("This slot stores one byte: only basic keys fit here.")
                .size(11.0)
                .color(theme::ORANGE),
        );
    }
    let mut picked: Option<u16> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
        for (name, cats) in SECTIONS {
            let entries = keys::section_entries(&app.catalog, cats, &app.search);
            if entries.is_empty() {
                continue;
            }
            ui.add_space(6.0);
            ui.label(RichText::new(name).size(12.0).color(theme::CYAN));
            ui.horizontal_wrapped(|ui| {
                for e in entries {
                    if tile(ui, &e.name, e.code, !only_basic || e.code <= 0xFF) {
                        picked = Some(e.code);
                    }
                }
            });
        }
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        if !only_basic {
            ui.add_space(8.0);
            ui.label(RichText::new("Mod-Tap").size(12.0).color(theme::CYAN));
            ui.horizontal(|ui| {
                ui.label(RichText::new("Mod:").size(11.0).color(theme::FG_MUTED));
                egui::ComboBox::from_id_salt("mt_mod").width(80.0).show_index(ui, &mut app.mt_mod, MOD_NAMES.len(), |i| MOD_NAMES[i]);
                ui.label(RichText::new("Key:").size(11.0).color(theme::FG_MUTED));
                egui::ComboBox::from_id_salt("mt_key").width(70.0).show_index(ui, &mut app.mt_key, MT_KEYS.len(), |i| MT_KEYS[i]);
                ui.label(
                    RichText::new(format!("= MT {} {}", MOD_NAMES[app.mt_mod], MT_KEYS[app.mt_key]))
                        .size(11.0)
                        .color(theme::GREEN),
                );
                if ui.button("Set").clicked() {
                    picked = Some(keys::mod_tap_code(app.mt_mod, app.mt_key));
                }
            });
            ui.add_space(8.0);
            ui.label(RichText::new("Layer-Tap").size(12.0).color(theme::CYAN));
            ui.horizontal(|ui| {
                ui.label(RichText::new("Layer:").size(11.0).color(theme::FG_MUTED));
                egui::ComboBox::from_id_salt("lt_layer").width(50.0).show_index(ui, &mut app.lt_layer, 10, |i| i.to_string());
                ui.label(RichText::new("Key:").size(11.0).color(theme::FG_MUTED));
                egui::ComboBox::from_id_salt("lt_key").width(70.0).show_index(ui, &mut app.lt_key, LT_KEYS.len(), |i| LT_KEYS[i]);
                ui.label(
                    RichText::new(format!("= LT {} {}", app.lt_layer, LT_KEYS[app.lt_key]))
                        .size(11.0)
                        .color(theme::GREEN),
                );
                if ui.button("Set").clicked() {
                    picked = Some(keys::layer_tap_code(app.lt_layer, app.lt_key));
                }
            });
            ui.add_space(8.0);
            ui.label(RichText::new("Mod + Key (one press)").size(12.0).color(theme::CYAN));
            ui.horizontal(|ui| {
                ui.label(RichText::new("Mod:").size(11.0).color(theme::FG_MUTED));
                egui::ComboBox::from_id_salt("mk_mod").width(80.0).show_index(ui, &mut app.mk_mod, MOD_NAMES.len(), |i| MOD_NAMES[i]);
                ui.label(RichText::new("Key:").size(11.0).color(theme::FG_MUTED));
                egui::ComboBox::from_id_salt("mk_key").width(70.0).show_index(ui, &mut app.mk_key, MT_KEYS.len(), |i| MT_KEYS[i]);
                let code = keys::mod_key_code(app.mk_mod, app.mk_key);
                ui.label(RichText::new(format!("= {}", keycode::decode_keycode(code))).size(11.0).color(theme::GREEN));
                if ui.button("Set").clicked() {
                    picked = Some(code);
                }
            });
            ui.add_space(8.0);
            ui.label(RichText::new("One-Shot Mod").size(12.0).color(theme::CYAN));
            ui.horizontal(|ui| {
                ui.label(RichText::new("Mod:").size(11.0).color(theme::FG_MUTED));
                egui::ComboBox::from_id_salt("osm_mod").width(80.0).show_index(ui, &mut app.osm_mod, ALL_MODS.len(), |i| ALL_MODS[i]);
                let code = keys::one_shot_code(app.osm_mod);
                ui.label(RichText::new(format!("= {}", keycode::decode_keycode(code))).size(11.0).color(theme::GREEN));
                if ui.button("Set").clicked() {
                    picked = Some(code);
                }
            });
            ui.add_space(8.0);
            ui.label(RichText::new("Layer + Mod").size(12.0).color(theme::CYAN));
            ui.horizontal(|ui| {
                ui.label(RichText::new("Layer:").size(11.0).color(theme::FG_MUTED));
                egui::ComboBox::from_id_salt("lm_layer").width(50.0).show_index(ui, &mut app.lm_layer, 10, |i| i.to_string());
                ui.label(RichText::new("Mod:").size(11.0).color(theme::FG_MUTED));
                egui::ComboBox::from_id_salt("lm_mod").width(80.0).show_index(ui, &mut app.lm_mod, ALL_MODS.len(), |i| ALL_MODS[i]);
                let code = keys::layer_mod_code(app.lm_layer, app.lm_mod);
                ui.label(RichText::new(format!("= {}", keycode::decode_keycode(code))).size(11.0).color(theme::GREEN));
                if ui.button("Set").clicked() {
                    picked = Some(code);
                }
            });
        }
        ui.add_space(8.0);
        ui.label(RichText::new("Hex Code").size(12.0).color(theme::CYAN));
        ui.horizontal(|ui| {
            ui.label(RichText::new("0x").color(theme::ORANGE));
            let edit = ui.add(egui::TextEdit::singleline(&mut app.hex_input).hint_text("5204").desired_width(80.0));
            let parsed = u16::from_str_radix(app.hex_input.trim(), 16).ok();
            if let Some(c) = parsed {
                ui.label(RichText::new(format!("= {}", keycode::decode_keycode(c))).size(11.0).color(theme::GREEN));
            }
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let ok = parsed.is_some_and(|c| !only_basic || c <= 0xFF);
            if (ui.add_enabled(ok, egui::Button::new("Set")).clicked() || enter) && ok {
                picked = parsed;
            }
        });
        ui.add_space(8.0);
        if ui.add(egui::Button::new(RichText::new("None (empty)").size(11.0)).min_size(vec2(120.0, 30.0))).clicked() {
            picked = Some(0);
        }
    });
    if let Some(code) = picked {
        app.pick_code(target, code);
        return true;
    }
    false
}
