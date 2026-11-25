//! Dracula palette, same values as the Slint theme of the previous version.

use egui::{Color32, CornerRadius, Stroke, Visuals};

const fn c(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const BG_PRIMARY: Color32 = c(0x28, 0x2a, 0x36);
pub const BG_SECONDARY: Color32 = c(0x44, 0x47, 0x5a);
pub const BG_SURFACE: Color32 = c(0x1e, 0x1e, 0x2e);
pub const FG_PRIMARY: Color32 = c(0xf8, 0xf8, 0xf2);
pub const FG_SECONDARY: Color32 = c(0x62, 0x72, 0xa4);
/// Muted text that stays legible on the secondary background.
pub const FG_MUTED: Color32 = c(0x9a, 0x9e, 0xbb);
pub const PURPLE: Color32 = c(0xbd, 0x93, 0xf9);
pub const GREEN: Color32 = c(0x50, 0xfa, 0x7b);
pub const CYAN: Color32 = c(0x8b, 0xe9, 0xfd);
pub const RED: Color32 = c(0xff, 0x55, 0x55);
pub const YELLOW: Color32 = c(0xf1, 0xfa, 0x8c);
pub const ORANGE: Color32 = c(0xff, 0xb8, 0x6c);
pub const BUTTON_BG: Color32 = c(0x56, 0x59, 0x70);
pub const BUTTON_HOVER: Color32 = c(0x62, 0x72, 0xa4);
pub const KEYCAP: Color32 = c(0x44, 0x47, 0x5a);
pub const DARK_TEXT: Color32 = c(0x1a, 0x1a, 0x2e);

/// Stepped heat scale of the original keycap (`key_button.slint`).
pub fn heat_color(heat: f32) -> Color32 {
    match heat {
        h if h > 0.8 => c(0xff, 0x00, 0x00),
        h if h > 0.6 => c(0xff, 0x44, 0x00),
        h if h > 0.4 => c(0xff, 0x88, 0x00),
        h if h > 0.2 => c(0xff, 0xcc, 0x00),
        h if h > 0.05 => c(0x44, 0x66, 0x88),
        _ => c(0x2d, 0x2d, 0x44),
    }
}

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.panel_fill = BG_PRIMARY;
    v.window_fill = BG_PRIMARY;
    v.extreme_bg_color = BG_PRIMARY;
    v.faint_bg_color = BG_SECONDARY;
    v.override_text_color = Some(FG_PRIMARY);
    v.hyperlink_color = CYAN;
    v.selection.bg_fill = PURPLE.linear_multiply(0.55);
    v.selection.stroke = Stroke::new(1.0, FG_PRIMARY);
    v.window_stroke = Stroke::new(1.0, PURPLE);
    let r = CornerRadius::same(4);
    for (w, bg, border) in [
        (&mut v.widgets.noninteractive, BG_SECONDARY, BG_PRIMARY),
        (&mut v.widgets.inactive, BUTTON_BG, BUTTON_HOVER),
        (&mut v.widgets.hovered, BUTTON_HOVER, BUTTON_HOVER),
        (&mut v.widgets.active, PURPLE, PURPLE),
        (&mut v.widgets.open, BUTTON_HOVER, PURPLE),
    ] {
        w.bg_fill = bg;
        w.weak_bg_fill = bg;
        w.bg_stroke = Stroke::new(1.0, border);
        w.corner_radius = r;
        w.fg_stroke = Stroke::new(1.0, FG_PRIMARY);
        w.expansion = 0.0;
    }
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.interact_size.y = 28.0;
        s.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::proportional(12.0),
        );
        s.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::proportional(12.0),
        );
    });
}
