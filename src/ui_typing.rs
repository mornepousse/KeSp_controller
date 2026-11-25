//! The Typing tab: how the keyboard is really used. The keyboard itself is the
//! main chart; numbers come as tiles and short lists around it.

use crate::app::App;
use crate::keys;
use crate::shape::{self, View};
use crate::theme;
use crate::ui_common::{card, note, page_head, paint_key, tile, CapStyle};
use egui::{vec2, Color32, Frame, RichText, Sense, Stroke};

/// Share of the busiest key to colour: one hue, from the keycap to full orange.
fn heat_fill(t: f32) -> Color32 {
    theme::KEYCAP.lerp_to_gamma(theme::ORANGE, t.clamp(0.0, 1.0))
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        Frame::new().inner_margin(16).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 10.0;
            let about = "What your fingers really do, from the counters in the keyboard.";
            if page_head(ui, "Typing", about, "Refresh", app.online()) {
                app.be.refresh_stats();
            }
            let total = app.stats.balance.as_ref().map_or(0, |b| b.total);
            let counted = app.stats.heat.first().map_or(0, Vec::len);
            let keymap_cols = app.doc.layer(0).and_then(|km| km.first()).map_or(0, Vec::len);
            if counted > 0 && counted < keymap_cols {
                ui.label(
                    RichText::new(format!(
                        "This keyboard only counts the {counted} columns of the half it scans: the other half's presses aren't in these numbers."
                    ))
                    .size(11.0)
                    .color(theme::ORANGE),
                );
            }
            if total == 0 {
                empty(app, ui);
                return;
            }
            tiles(app, ui);
            heat_keyboard(app, ui);
            ui.columns(2, |c| {
                c[0].spacing_mut().item_spacing.y = 10.0;
                c[1].spacing_mut().item_spacing.y = 10.0;
                fingers(app, &mut c[0]);
                rows(app, &mut c[0]);
                top_keys(app, &mut c[1]);
                pet(app, &mut c[1]);
            });
            dead_keys(app, ui);
        });
    });
}

fn empty(app: &App, ui: &mut egui::Ui) {
    card(ui, |ui| {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            let (title, body) = if app.stats.balance.is_some() {
                ("No presses counted yet", "Type for a while, then press Refresh.")
            } else if app.online() {
                ("Nothing read yet", "Press Refresh to read the keyboard's counters.")
            } else {
                ("No keyboard connected", "Connect one to see its counters.")
            };
            ui.label(RichText::new(title).size(16.0));
            note(ui, body);
        });
        ui.add_space(30.0);
    });
}

fn tiles(app: &App, ui: &mut egui::Ui) {
    let Some(b) = &app.stats.balance else { return };
    ui.columns(4, |c| {
        tile(&mut c[0], "Presses", &b.total.to_string(), "counted so far");
        tile(&mut c[1], "Left / Right", &format!("{:.0} / {:.0}", b.left_pct, b.right_pct), "% of presses");
        match app.stats.bigrams.as_ref().filter(|g| g.total > 0) {
            Some(g) => tile(
                &mut c[2],
                "Same finger",
                &format!("{:.1}%", g.sfb_pct),
                &format!("of key pairs, {:.0}% alternate hands", g.alt_hand_pct),
            ),
            None => tile(&mut c[2], "Same finger", "—", "no key pairs counted"),
        }
        let wpm = if app.wpm > 0 { app.wpm.to_string() } else { "—".to_string() };
        tile(&mut c[3], "Speed", &wpm, "words per minute now");
    });
}

fn heat_keyboard(app: &App, ui: &mut egui::Ui) {
    let (area, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 270.0), Sense::hover());
    ui.painter().rect_filled(area, 8.0, theme::BG_SURFACE);
    let painter = ui.painter_at(area);
    let view = View::fit(&app.keys, area.shrink2(vec2(8.0, 14.0)));
    let max = app.stats.heat_max.max(1) as f32;
    let count_of = |r: usize, c: usize| app.stats.heat.get(r).and_then(|row| row.get(c)).copied();
    for k in &app.keys {
        let count = count_of(k.row, k.col);
        let t = count.map_or(0.0, |n| n as f32 / max);
        let fill = if count.is_some() { heat_fill(t) } else { theme::BG_SECONDARY.gamma_multiply(0.4) };
        let text = if t > 0.55 { theme::DARK_TEXT } else if count.is_some() { theme::FG_PRIMARY } else { theme::FG_SECONDARY };
        let label = app
            .doc
            .get((app.layer, k.row, k.col))
            .map(|c| keys::label_for(c, &app.kb_layout))
            .unwrap_or_default();
        let style = CapStyle { fill, stroke: Stroke::new(1.0, theme::BG_PRIMARY), text };
        paint_key(&painter, &view, k, &style, &label);
    }
    // Scale, bottom right: fewer to more.
    let w = 18.0;
    let right = area.right() - 12.0;
    let y = area.bottom() - 18.0;
    for i in 0..5 {
        let r = egui::Rect::from_min_size(egui::pos2(right - (5 - i) as f32 * (w + 2.0), y), vec2(w, 8.0));
        painter.rect_filled(r, 2.0, heat_fill(i as f32 / 4.0));
    }
    let font = egui::FontId::proportional(10.0);
    painter.text(
        egui::pos2(right - 5.0 * (w + 2.0) - 6.0, y + 4.0),
        egui::Align2::RIGHT_CENTER,
        "fewer",
        font.clone(),
        theme::FG_MUTED,
    );
    painter.text(egui::pos2(right, y - 8.0), egui::Align2::RIGHT_CENTER, "more", font, theme::FG_MUTED);

    if let Some(i) = resp.hover_pos().and_then(|p| shape::key_at(&view, &app.keys, p)) {
        let k = &app.keys[i];
        let total = app.stats.balance.as_ref().map_or(0, |b| b.total).max(1);
        let tip = match count_of(k.row, k.col) {
            Some(n) => format!("R{}C{}\n{n} presses\n{:.1}% of all", k.row, k.col, n as f32 / total as f32 * 100.0),
            None => format!("R{}C{}\nnot counted by this keyboard", k.row, k.col),
        };
        resp.on_hover_text_at_pointer(tip);
    }
}

/// Bars growing away from the middle: the two hands face each other.
fn fingers(app: &App, ui: &mut egui::Ui) {
    if app.stats.fingers.len() < 10 {
        return;
    }
    card(ui, |ui| {
        ui.label(RichText::new("Fingers").size(13.0).color(theme::CYAN));
        let top = app.stats.fingers.iter().map(|f| f.pct).fold(1.0f32, f32::max);
        for i in 0..5 {
            let (l, r) = (&app.stats.fingers[i], &app.stats.fingers[i + 5]);
            let name = l.name.trim_end_matches(" L");
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
            let p = ui.painter();
            let mid = rect.center().x;
            let half_label = 28.0;
            let span = (rect.width() / 2.0 - half_label - 44.0).max(10.0);
            let font = egui::FontId::proportional(11.0);
            p.text(rect.center(), egui::Align2::CENTER_CENTER, name, font.clone(), theme::FG_MUTED);
            let y0 = rect.center().y - 6.0;
            let lw = span * l.pct / top;
            let rw = span * r.pct / top;
            let lr = egui::Rect::from_min_size(egui::pos2(mid - half_label - lw, y0), vec2(lw, 12.0));
            let rr = egui::Rect::from_min_size(egui::pos2(mid + half_label, y0), vec2(rw, 12.0));
            p.rect_filled(lr, 3.0, theme::PURPLE);
            p.rect_filled(rr, 3.0, theme::PURPLE);
            p.text(
                egui::pos2(lr.left() - 4.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                format!("{:.1}%", l.pct),
                font.clone(),
                theme::FG_PRIMARY,
            );
            p.text(
                egui::pos2(rr.right() + 4.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                format!("{:.1}%", r.pct),
                font,
                theme::FG_PRIMARY,
            );
            resp.on_hover_text(format!("{name}: left {} presses, right {} presses", l.count, r.count));
        }
        ui.horizontal(|ui| {
            note(ui, "left hand");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| note(ui, "right hand"));
        });
    });
}

fn rows(app: &App, ui: &mut egui::Ui) {
    card(ui, |ui| {
        ui.label(RichText::new("Rows").size(13.0).color(theme::CYAN));
        let top = app.stats.rows.iter().map(|r| r.pct).fold(1.0f32, f32::max);
        for r in &app.stats.rows {
            list_bar(ui, &r.name, "", r.pct / top, &format!("{:.1}%", r.pct), &format!("{} presses", r.count));
        }
    });
}

fn top_keys(app: &App, ui: &mut egui::Ui) {
    card(ui, |ui| {
        ui.label(RichText::new("Busiest keys").size(13.0).color(theme::CYAN));
        let top = app.stats.top.first().map_or(1, |k| k.count).max(1) as f32;
        for k in &app.stats.top {
            list_bar(ui, &k.name, &k.finger, k.count as f32 / top, &k.count.to_string(), &format!("{:.1}% of all", k.pct));
        }
    });
}

/// Name, optional detail, a thin bar and its value.
fn list_bar(ui: &mut egui::Ui, name: &str, detail: &str, share: f32, value: &str, tip: &str) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    let p = ui.painter();
    let font = egui::FontId::proportional(11.0);
    p.text(rect.left_center(), egui::Align2::LEFT_CENTER, name, font.clone(), theme::FG_PRIMARY);
    let x0 = if detail.is_empty() {
        rect.left() + 56.0
    } else {
        p.text(
            rect.left_center() + vec2(72.0, 0.0),
            egui::Align2::LEFT_CENTER,
            detail,
            egui::FontId::proportional(10.0),
            theme::FG_MUTED,
        );
        rect.left() + 136.0
    };
    let x1 = (rect.right() - 48.0).max(x0 + 8.0);
    let w = (x1 - x0) * share.clamp(0.0, 1.0);
    let bar = egui::Rect::from_min_size(egui::pos2(x0, rect.center().y - 5.0), vec2(w, 10.0));
    p.rect_filled(bar, 3.0, theme::PURPLE);
    p.text(rect.right_center(), egui::Align2::RIGHT_CENTER, value, font, theme::FG_PRIMARY);
    resp.on_hover_text(tip);
}

fn pet(app: &App, ui: &mut egui::Ui) {
    let Some(t) = &app.stats.tama else { return };
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Tamagotchi").size(13.0).color(theme::CYAN));
            let s = if t.enabled { "on" } else { "off" };
            ui.label(RichText::new(s).size(11.0).color(theme::FG_MUTED));
        });
        let needed = 500 + t.level as u32 * 250;
        note(
            ui,
            &format!("Level {}, {} / {} XP, {} keys, top {} per minute", t.level, t.xp, needed, t.total_keys, t.max_kpm),
        );
        for (name, v) in [("Hunger", t.hunger), ("Happy", t.happiness), ("Energy", t.energy), ("Health", t.health)] {
            let pct = v as f32 / 10.0;
            list_bar(ui, name, "", pct / 100.0, &format!("{pct:.0}%"), name);
        }
    });
}

fn dead_keys(app: &App, ui: &mut egui::Ui) {
    if app.stats.dead.is_empty() {
        return;
    }
    card(ui, |ui| {
        ui.label(RichText::new("Never pressed").size(13.0).color(theme::CYAN));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for d in &app.stats.dead {
                Frame::new()
                    .fill(theme::BG_PRIMARY)
                    .corner_radius(3)
                    .inner_margin(egui::Margin::symmetric(6, 3))
                    .show(ui, |ui| ui.label(RichText::new(d).size(10.0)));
            }
        });
    });
}
