//! Small shared pieces: cards, bars, section titles, and drawing a keycap.

use crate::protocol::layout::KeycapPos;
use crate::shape::View;
use crate::theme;
use egui::{Color32, CornerRadius, Frame, Pos2, Rect, RichText, Stroke, StrokeKind};

pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    Frame::new()
        .fill(theme::BG_SECONDARY)
        .corner_radius(8)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Page title, one line saying what it is for, and an optional action on the right.
/// True when the action is clicked. An empty `button` means no action.
pub fn page_head(ui: &mut egui::Ui, text: &str, about: &str, button: &str, enabled: bool) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        title(ui, text);
        if !button.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                clicked = ui.add_enabled(enabled, egui::Button::new(button)).clicked();
            });
        }
    });
    note(ui, about);
    clicked
}

/// Pages of a tab, down the left side. A count beside a name when there is one.
pub fn side_nav<T: Copy + PartialEq>(ui: &mut egui::Ui, id: &str, items: &[(T, &str, usize)], current: &mut T) {
    egui::Panel::left(id.to_string())
        .exact_size(164.0)
        .frame(Frame::new().fill(theme::BG_SURFACE).inner_margin(8))
        .show_separator_line(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for (value, name, count) in items {
                let active = *current == *value;
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::click());
                let fill = if active {
                    theme::BG_SECONDARY
                } else if resp.hovered() {
                    theme::BG_PRIMARY
                } else {
                    Color32::TRANSPARENT
                };
                let p = ui.painter();
                p.rect_filled(rect, 4.0, fill);
                if active {
                    let bar = Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height()));
                    p.rect_filled(bar, 1.5, theme::PURPLE);
                }
                let color = if active { theme::FG_PRIMARY } else { theme::FG_MUTED };
                let font = egui::FontId::proportional(13.0);
                p.text(rect.left_center() + egui::vec2(12.0, 0.0), egui::Align2::LEFT_CENTER, *name, font.clone(), color);
                if *count > 0 {
                    p.text(
                        rect.right_center() - egui::vec2(8.0, 0.0),
                        egui::Align2::RIGHT_CENTER,
                        count.to_string(),
                        egui::FontId::proportional(11.0),
                        theme::FG_MUTED,
                    );
                }
                if resp.clicked() {
                    *current = *value;
                }
            }
        });
}

/// A headline number: name above, big value, short explanation under.
pub fn tile(ui: &mut egui::Ui, name: &str, value: &str, detail: &str) {
    Frame::new()
        .fill(theme::BG_SECONDARY)
        .corner_radius(8)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(name).size(11.0).color(theme::FG_MUTED));
            ui.label(RichText::new(value).size(22.0));
            ui.add(egui::Label::new(RichText::new(detail).size(10.0).color(theme::FG_MUTED)).wrap());
        });
}

pub fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(20.0).strong());
}

pub fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(14.0).color(theme::CYAN));
}

pub fn note(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(theme::FG_MUTED));
}

pub fn primary(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).color(theme::BG_PRIMARY)).fill(theme::PURPLE)
}

pub fn progress(ui: &mut egui::Ui, value: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 20.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 4.0, theme::BG_PRIMARY);
    let mut fill = rect;
    fill.set_width(rect.width() * value.clamp(0.0, 1.0));
    p.rect_filled(fill, 4.0, if value >= 1.0 { theme::GREEN } else { theme::PURPLE });
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        format!("{:.0}%", value * 100.0),
        egui::FontId::proportional(11.0),
        theme::FG_PRIMARY,
    );
}

/// How to paint one keycap.
pub struct CapStyle {
    pub fill: Color32,
    pub stroke: Stroke,
    pub text: Color32,
}

/// Draw a key rotated around its centre, with its label kept inside it: the
/// label shrinks, then breaks on a space, and stays upright relative to the key.
pub fn paint_key(painter: &egui::Painter, view: &View, k: &KeycapPos, style: &CapStyle, label: &str) {
    let rect = view.key_rect(k);
    let angle = k.angle.to_radians();
    painter.add(
        egui::epaint::RectShape::new(
            rect.shrink(view.scale.min(1.0)),
            CornerRadius::same(4),
            style.fill,
            style.stroke,
            StrokeKind::Inside,
        )
        .with_angle(angle),
    );

    let max_w = (rect.width() - 4.0).max(6.0);
    let base = (11.0 * view.scale).clamp(7.0, 15.0);
    let lines: Vec<&str> = {
        let one_fits = painter
            .layout_no_wrap(label.to_string(), egui::FontId::proportional(base), style.text)
            .size()
            .x
            <= max_w;
        match label.split_once(' ') {
            Some((a, b)) if !one_fits => vec![a, b],
            _ => vec![label],
        }
    };
    let longest = lines
        .iter()
        .map(|l| {
            painter
                .layout_no_wrap(l.to_string(), egui::FontId::proportional(base), style.text)
                .size()
                .x
        })
        .fold(0.0f32, f32::max);
    let size = if longest > max_w { (base * max_w / longest).max(6.0) } else { base };
    let line_h = size * 1.15;
    let top = -(lines.len() as f32 - 1.0) * line_h / 2.0;
    let rot = egui::emath::Rot2::from_angle(angle);
    for (i, line) in lines.iter().enumerate() {
        let galley = painter.layout_no_wrap(line.to_string(), egui::FontId::proportional(size), style.text);
        let offset = rot * egui::vec2(0.0, top + i as f32 * line_h);
        let center: Pos2 = rect.center() + offset;
        let at = center - galley.size() / 2.0;
        painter.add(
            egui::epaint::TextShape::new(at, galley, style.text)
                .with_angle_and_anchor(angle, egui::Align2::CENTER_CENTER),
        );
    }
}
