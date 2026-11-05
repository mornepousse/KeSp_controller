//! The Device tab: the keyboard as a piece of hardware. Its state, its radio link,
//! backups, firmware, the matrix test and the resets.

use crate::app::{Action, App, Confirm, DevicePage};
use crate::backend::{self, PickTarget};
use crate::shape::View;
use crate::theme;
use crate::ui_common::{card, note, page_head, paint_key, primary, progress, section, side_nav, CapStyle};
use egui::{vec2, Frame, RichText, Sense, Stroke};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let items = [
        (DevicePage::Status, "Status", 0),
        (DevicePage::Backup, "Backup", 0),
        (DevicePage::Firmware, "Firmware", 0),
        (DevicePage::MatrixTest, "Matrix Test", 0),
        (DevicePage::Reset, "Reset", 0),
    ];
    let mut current = app.device_page;
    side_nav(ui, "device_nav", &items, &mut current);
    app.device_page = current;
    egui::CentralPanel::default().frame(Frame::new()).show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            Frame::new().inner_margin(16).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 10.0;
                match app.device_page {
                    DevicePage::Status => status(app, ui),
                    DevicePage::Backup => backup(app, ui),
                    DevicePage::Firmware => firmware(app, ui),
                    DevicePage::MatrixTest => matrix(app, ui),
                    DevicePage::Reset => reset(app, ui),
                }
            });
        });
    });
}

fn fact(ui: &mut egui::Ui, name: &str, value: &str) {
    ui.label(RichText::new(name).size(11.0).color(theme::FG_MUTED));
    ui.label(RichText::new(value).size(12.0));
    ui.end_row();
}

fn status(app: &mut App, ui: &mut egui::Ui) {
    if page_head(ui, "Status", "What is plugged in, and how it is doing.", "Refresh All", app.online() && app.is_dongle) {
        app.be.dongle_refresh();
    }
    card(ui, |ui| {
        egui::Grid::new("facts").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
            if app.online() {
                let name = if app.device_name.is_empty() { "Unknown" } else { app.device_name.as_str() };
                fact(ui, "Keyboard", name);
                fact(ui, "Connection", &app.mode_label);
                fact(ui, "Firmware", &app.firmware);
                let layers = app.layer_names.len();
                fact(ui, "Keys", &format!("{} keys, {layers} layers", app.keys.len()));
            } else {
                fact(ui, "Keyboard", "None connected");
                fact(ui, "Drawn as", app.boards.get(app.board).map_or("", |b| b.name.as_str()));
            }
        });
    });
    if !app.is_dongle {
        return;
    }
    card(ui, |ui| {
        ui.label(RichText::new("Radio link").size(13.0).color(theme::CYAN));
        let rf = &app.dongle.rf;
        ui.columns(2, |c| {
            link_side(&mut c[0], "Left half", rf.link_left, rf.sig_left, rf.hb_age_left_ms, rf.pkt_rx_left, rf.pkt_dup_left);
            link_side(&mut c[1], "Right half", rf.link_right, rf.sig_right, rf.hb_age_right_ms, rf.pkt_rx_right, rf.pkt_dup_right);
        });
    });
    card(ui, |ui| {
        ui.label(RichText::new("Battery").size(13.0).color(theme::CYAN));
        let b = app.dongle.battery.as_ref();
        ui.columns(2, |c| {
            battery_side(&mut c[0], "Left half", b.map(|b| &b.left));
            battery_side(&mut c[1], "Right half", b.map(|b| &b.right));
        });
    });
    card(ui, |ui| {
        ui.label(RichText::new("Pairing").size(13.0).color(theme::CYAN));
        note(ui, &format!("Paired halves: {} / 2   L: {}   R: {}", app.dongle.paired, app.dongle.mac_left, app.dongle.mac_right));
        ui.horizontal(|ui| {
            let active = app.dongle.pair_countdown;
            let label = match active {
                Some(s) => format!("Waiting… {s} s"),
                None => format!("Open pairing window ({} s)", backend::PAIR_WINDOW_S),
            };
            let usable = active.is_none() && app.online();
            if ui.add_enabled(usable, egui::Button::new(label)).clicked() {
                app.dongle.pair_status.clear();
                app.be.dongle_pair();
            }
            if ui.add_enabled(usable, egui::Button::new("Reset paired")).clicked() {
                app.confirm = Some(Confirm {
                    title: "Forget the paired halves?".into(),
                    body: "The dongle forgets both halves. They will have to be paired again.".into(),
                    ok: "Forget them".into(),
                    action: Action::ResetPairing,
                });
            }
        });
        if !app.dongle.pair_status.is_empty() {
            note(ui, &app.dongle.pair_status);
        }
    });
}

fn signal_bars(ui: &mut egui::Ui, bars: u8) {
    let (rect, _) = ui.allocate_exact_size(vec2(24.0, 12.0), Sense::hover());
    for i in 0..4u8 {
        let h = 6.0 + i as f32 * 2.0;
        let x = rect.left() + i as f32 * 6.0;
        let r = egui::Rect::from_min_max(egui::pos2(x, rect.bottom() - h), egui::pos2(x + 4.0, rect.bottom()));
        ui.painter().rect_filled(r, 1.0, if i < bars { theme::GREEN } else { theme::BG_PRIMARY });
    }
}

fn link_side(ui: &mut egui::Ui, name: &str, up: bool, sig: u8, hb_ms: u32, rx: u32, dup: u32) {
    let bars = crate::protocol::parsers::signal_bars(sig);
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).size(13.0));
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 5.0, if up { theme::GREEN } else { theme::RED });
        note(ui, if up { "linked" } else { "not linked" });
        signal_bars(ui, bars);
    });
    note(ui, &format!("Signal {sig}/255, last heard {} s ago", hb_ms / 1000));
    note(ui, &format!("{rx} packets, {dup} duplicates"));
}

fn battery_side(ui: &mut egui::Ui, name: &str, side: Option<&crate::protocol::parsers::HalfBattery>) {
    ui.label(RichText::new(name).size(13.0));
    match side.filter(|s| s.has_value()) {
        None => {
            ui.label(RichText::new("—").size(18.0));
            note(ui, "no reading yet");
        }
        Some(s) => {
            let bolt = if s.charging == 1 { "⚡ " } else { "" };
            let color = if s.is_stale() { theme::FG_MUTED } else { theme::FG_PRIMARY };
            ui.label(RichText::new(format!("{bolt}{}%", s.soc_pct)).size(18.0).color(color));
            let age = s.age_ms / 1000;
            let when = if s.is_stale() { format!("old reading, {age} s ago") } else { format!("{age} s ago") };
            note(ui, &format!("{:.1} V, {when}", s.batt_dv as f32 / 10.0));
        }
    }
}

fn backup(app: &mut App, ui: &mut egui::Ui) {
    page_head(ui, "Backup", "Save the keyboard's setup to a file, or put one back.", "", false);
    card(ui, |ui| {
        note(ui, "What to include. Nothing checked means everything.");
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut app.config_mask[0], "Keymaps + Layers");
            ui.checkbox(&mut app.config_mask[1], "Macros");
            ui.checkbox(&mut app.config_mask[2], "TD / Combos / Leaders / KO");
        });
        ui.horizontal(|ui| {
            let ok = !app.config.busy && app.online();
            let (e, i) = if app.config.busy { ("Working...", "Working...") } else { ("Export", "Import") };
            if ui.add_enabled(ok, primary(e)).clicked() {
                app.be.pick(PickTarget::ConfigExport);
            }
            if ui.add_enabled(ok, egui::Button::new(i)).clicked() {
                app.be.pick(PickTarget::ConfigImport);
            }
            ui.label(&app.config.status);
        });
        if app.config.busy {
            progress(ui, app.config.progress);
        }
    });
}

fn firmware(app: &mut App, ui: &mut egui::Ui) {
    page_head(ui, "Firmware", "Put a new firmware on the keyboard.", "", false);
    if !backend::CAN_FLASH {
        card(ui, |ui| {
            ui.label("Firmware updates need the desktop app.");
            note(ui, "A failed update in a browser tab can leave a keyboard unusable, so this version doesn't offer it.");
        });
        return;
    }
    card(ui, |ui| {
        section(ui, "Over USB");
        note(ui, "No programming cable needed.");
        ui.horizontal(|ui| {
            if ui.button("Browse...").clicked() {
                app.be.pick(PickTarget::Ota);
            }
            let (text, color) = if app.ota.path.is_empty() {
                ("No firmware file selected", theme::FG_MUTED)
            } else {
                (app.ota.path.as_str(), theme::FG_PRIMARY)
            };
            ui.add(egui::Label::new(RichText::new(text).color(color)).truncate());
        });
        ui.horizontal(|ui| {
            let label = if app.ota.busy { "Flashing..." } else { "Flash OTA" };
            let ok = !app.ota.busy && !app.ota.path.is_empty() && app.online();
            if ui.add_enabled(ok, primary(label)).clicked() {
                app.confirm = Some(Confirm {
                    title: "Update the keyboard's firmware?".into(),
                    body: format!("Sends {} to the keyboard over USB. Don't unplug it until it reboots.", app.ota.path),
                    ok: "Flash".into(),
                    action: Action::Ota(app.ota.path.clone()),
                });
            }
            ui.label(&app.ota.status);
        });
        if app.ota.busy || app.ota.progress > 0.0 {
            progress(ui, app.ota.progress);
        }
    });
    card(ui, |ui| {
        section(ui, "ESP32 flasher");
        note(ui, "Through the programming port (CH340/CP2102).");
        ui.horizontal(|ui| {
            if app.tools.prog_ports.is_empty() {
                ui.add(egui::TextEdit::singleline(&mut app.tools.prog_port).hint_text("/dev/ttyUSB0").desired_width(240.0));
            } else {
                let ports = app.tools.prog_ports.clone();
                egui::ComboBox::from_id_salt("prog_port")
                    .width(240.0)
                    .selected_text(app.tools.prog_port.clone())
                    .show_ui(ui, |ui| {
                        for p in &ports {
                            ui.selectable_value(&mut app.tools.prog_port, p.clone(), p);
                        }
                    });
            }
            if ui.button("Refresh").clicked() {
                app.tools.prog_ports = backend::list_prog_ports();
                if let Some(first) = app.tools.prog_ports.first() {
                    if !app.tools.prog_ports.contains(&app.tools.prog_port) {
                        app.tools.prog_port = first.clone();
                    }
                }
            }
        });
        if app.tools.prog_ports.is_empty() {
            ui.label(RichText::new("No CH340/CP210x port detected. Enter path manually.").size(11.0).color(theme::YELLOW));
        }
        const OFFSETS: [(&str, u32); 3] =
            [("full (0x0)", 0x0), ("factory (0x20000)", 0x20000), ("ota_0 (0x220000)", 0x220000)];
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("flash_offset").width(170.0).show_index(
                ui,
                &mut app.tools.flash_offset,
                OFFSETS.len(),
                |i| OFFSETS[i].0,
            );
            if ui.button("Browse...").clicked() {
                app.be.pick(PickTarget::Firmware);
            }
            let (text, color) = if app.flash.path.is_empty() {
                ("No file selected", theme::FG_MUTED)
            } else {
                (app.flash.path.as_str(), theme::FG_PRIMARY)
            };
            ui.add(egui::Label::new(RichText::new(text).color(color)).truncate());
        });
        ui.horizontal(|ui| {
            let label = if app.flash.busy { "Flashing..." } else { "Flash" };
            let ok = !app.flash.busy && !app.flash.path.is_empty() && !app.tools.prog_port.is_empty();
            if ui.add_enabled(ok, primary(label)).clicked() {
                let (name, offset) = OFFSETS[app.tools.flash_offset];
                app.confirm = Some(Confirm {
                    title: "Flash this firmware?".into(),
                    body: format!(
                        "Writes {} to {} at {}. Check that this port really is your keyboard's programming cable.",
                        app.flash.path, app.tools.prog_port, name
                    ),
                    ok: "Flash".into(),
                    action: Action::Flash {
                        port: app.tools.prog_port.clone(),
                        path: app.flash.path.clone(),
                        offset,
                    },
                });
            }
            ui.label(&app.flash.status);
        });
        if app.flash.busy || app.flash.progress > 0.0 {
            progress(ui, app.flash.progress);
        }
    });
    Frame::new()
        .fill(theme::RED.gamma_multiply(0.13))
        .stroke(Stroke::new(1.0, theme::RED))
        .corner_radius(8)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            note(ui, "Do not disconnect the keyboard during flashing. It reboots by itself when done.");
        });
}

fn matrix(app: &mut App, ui: &mut egui::Ui) {
    let label = if app.tools.matrix_on { "Stop Test" } else { "Start Test" };
    if page_head(ui, "Matrix Test", "Press each key: it lights up if the keyboard sees it.", label, app.online()) {
        app.be.toggle_matrix();
    }
    if !app.tools.matrix_status.is_empty() {
        let c = if app.tools.matrix_on { theme::GREEN } else { theme::FG_MUTED };
        ui.label(RichText::new(&app.tools.matrix_status).size(11.0).color(c));
    }
    let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), 280.0), Sense::hover());
    ui.painter().rect_filled(area, 8.0, theme::BG_SURFACE);
    let painter = ui.painter_at(area);
    let view = View::fit(&app.keys, area);
    for k in &app.keys {
        let fill = app.tools.matrix_colors.get(&(k.row, k.col)).copied().unwrap_or(theme::KEYCAP);
        let text = if fill == theme::KEYCAP { theme::FG_PRIMARY } else { theme::BG_PRIMARY };
        let style = CapStyle { fill, stroke: Stroke::new(1.0, theme::BG_PRIMARY), text };
        paint_key(&painter, &view, k, &style, &format!("R{}C{}", k.row, k.col));
    }
    if app.tools.matrix_on || !app.tools.matrix_log.is_empty() {
        Frame::new().fill(theme::BG_SURFACE).corner_radius(4).inner_margin(8).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(90.0);
            for line in &app.tools.matrix_log {
                ui.label(RichText::new(line).monospace().size(10.0).color(theme::FG_MUTED));
            }
        });
    }
}

const NVS_NAMES: [(&str, u8); 6] = [
    ("Keymaps + Layers", 0x01),
    ("Macros", 0x02),
    ("Stats (keys + bigrams)", 0x04),
    ("TD / Combos / Leaders / KO", 0x08),
    ("Bluetooth", 0x10),
    ("Tamagotchi", 0x20),
];

fn reset(app: &mut App, ui: &mut egui::Ui) {
    page_head(ui, "Reset", "Erase what the keyboard saved. It reboots with its defaults.", "", false);
    card(ui, |ui| {
        egui::Grid::new("nvs").num_columns(2).show(ui, |ui| {
            for i in 0..3 {
                ui.checkbox(&mut app.tools.nvs[i], NVS_NAMES[i].0);
                ui.checkbox(&mut app.tools.nvs[i + 3], NVS_NAMES[i + 3].0);
                ui.end_row();
            }
        });
        ui.horizontal(|ui| {
            let mask = NVS_NAMES
                .iter()
                .zip(app.tools.nvs.iter())
                .fold(0u8, |m, ((_, bit), on)| if *on { m | bit } else { m });
            if ui.add_enabled(app.online() && mask != 0, egui::Button::new("Reset selected")).clicked() {
                app.confirm = Some(Confirm {
                    title: "Erase the selected settings?".into(),
                    body: "They are erased from the keyboard for good and it reboots with its defaults.".into(),
                    ok: "Erase".into(),
                    action: Action::NvsReset(mask),
                });
            }
            if ui.add_enabled(app.online(), egui::Button::new("Factory reset")).clicked() {
                app.confirm = Some(Confirm {
                    title: "Factory reset?".into(),
                    body: "Everything saved on the keyboard is erased for good: keymaps, macros, stats, pairings. Export a backup first if you want to keep them.".into(),
                    ok: "Erase everything".into(),
                    action: Action::NvsReset(0xFF),
                });
            }
            note(ui, &app.tools.nvs_status);
        });
    });
    ui.add_space(8.0);
    note(ui, concat!("KeSp Controller v", env!("CARGO_PKG_VERSION"), ", made with egui"));
}
