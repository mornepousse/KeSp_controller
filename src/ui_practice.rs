//! The Practice tab: touch typing on this keyboard, with its own keymap.

use crate::app::{Action, App, Confirm};
use crate::keys;
use crate::practice::{self, Lang, Mode, Session, Typed};
use crate::practice_game::{self as game, BADGES, DAILY_GOAL};
use crate::protocol::{settings, stats};
use crate::shape::View;
use crate::theme;
use crate::ui_common::{card, note, page_head, paint_key, tile, CapStyle};
use egui::text::{LayoutJob, TextFormat};
use egui::{vec2, Color32, FontId, Frame, RichText, Sense, Stroke};

/// Tests start from nothing and leave no file behind.
fn load_save() -> practice::Save {
    if cfg!(test) {
        return practice::Save::default();
    }
    serde_json::from_value(settings::load().practice).unwrap_or_default()
}

pub fn store(s: &Session) {
    if cfg!(test) {
        return;
    }
    let mut all = settings::load();
    if let Ok(v) = serde_json::to_value(&s.save) {
        all.practice = v;
        settings::save(&all);
    }
}

fn seed() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64)
}

/// Red for a slow letter, green once it reaches the target.
fn mastery(confidence: f32) -> Color32 {
    let t = confidence.clamp(0.0, 1.0);
    if t < 0.5 {
        theme::RED.lerp_to_gamma(theme::YELLOW, t * 2.0)
    } else {
        theme::YELLOW.lerp_to_gamma(theme::GREEN, (t - 0.5) * 2.0)
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let mut s = app.practice.take().unwrap_or_else(|| Session::new(load_save(), seed()));
    s.today = game::today();
    let places = practice::places(&app.all_keys(), &app.kb_layout);
    s.set_alphabet(practice::alphabet(s.lang(), &places), practice::symbols(&places));
    s.same_finger = same_finger_pairs(app, &s.alphabet, &places);
    let shift = practice::shift_key(&app.all_keys());
    s.has_shift = shift.is_some();
    take_input(&mut s, ui);

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        Frame::new().inner_margin(16).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 10.0;
            let about = "Touch typing on your own keymap: letters one at a time as on keybr.com, real words, \
                         your slowest letter pairs, the signs of your layers, or a 60-second sprint.";
            if page_head(ui, "Practice", about, "New line", !s.text.is_empty()) {
                s.new_line();
            }
            options(app, &mut s, ui);
            game_banner(&s, ui);
            if !s.can_practise() {
                card(ui, |ui| {
                    ui.label(RichText::new("Nothing to type on yet").size(16.0));
                    let why = if s.mode() == Mode::Symbols {
                        "This keymap has too few signs this computer's layout can name: the app reads a key's sign from its label, and shifted signs only on QWERTY."
                    } else {
                        "This keymap has too few of this language's letters. Connect a keyboard, or pick a model while none is connected."
                    };
                    note(ui, why);
                });
                return;
            }
            letters_strip(&s, ui);
            typing_line(&s, ui);
            next_key_hint(app, &s, &places, shift, ui);
            if s.save.show_keyboard {
                keyboard(app, &s, &places, shift, ui);
            }
            results(&s, ui);
            note(ui, "Type on the keyboard: the app reads what the computer receives. Keys are labelled with the layout chosen at the bottom of the Keymap tab.");
        });
    });
    // Live speed and the red flash both need frames while nothing is typed.
    if s.pos > 0 || s.sprint_start.is_some() || ui.input(|i| i.time) < s.wrong_until {
        ui.ctx().request_repaint_after(web_time::Duration::from_millis(200));
    }
    app.practice = Some(s);
}

fn take_input(s: &mut Session, ui: &egui::Ui) {
    if s.tick(ui.input(|i| i.time)) {
        store(s);
    }
    // A field being edited (the target speed) keeps its own typing.
    if ui.ctx().memory(|m| m.focused().is_some()) {
        return;
    }
    let (events, now) = ui.input(|i| (i.events.clone(), i.time));
    let mut done = false;
    for e in events {
        if let egui::Event::Text(t) = e {
            for c in t.chars() {
                done |= s.type_char(c, now) == Typed::LineDone;
            }
        }
    }
    if done {
        store(s);
    }
}

fn options(app: &mut App, s: &mut Session, ui: &mut egui::Ui) {
    // Wraps on a narrow window instead of pushing the whole page wider.
    ui.horizontal_wrapped(|ui| {
        let mode = s.mode();
        for m in Mode::ALL {
            if ui.selectable_label(m == mode, m.name()).clicked() && m != mode {
                s.set_mode(m);
                store(s);
            }
        }
        ui.separator();
        let lang = s.lang();
        egui::ComboBox::from_id_salt("practice_lang").selected_text(lang.name()).show_ui(ui, |ui| {
            for l in Lang::ALL {
                if ui.selectable_label(l == lang, l.name()).clicked() && l != lang {
                    s.set_lang(l);
                    store(s);
                }
            }
        });
        ui.label("Target");
        let before = s.save.target_wpm;
        ui.add(egui::DragValue::new(&mut s.save.target_wpm).range(10..=200).suffix(" wpm"));
        let was = (s.save.show_keyboard, s.save.show_fingers, s.save.hide_labels);
        ui.checkbox(&mut s.save.show_keyboard, "Show keyboard");
        let on = s.save.show_keyboard;
        ui.add_enabled(on, egui::Checkbox::new(&mut s.save.show_fingers, "Fingers"))
            .on_hover_text("Outline each key in the colour of the finger that types it");
        ui.add_enabled(on, egui::Checkbox::new(&mut s.save.hide_labels, "Blank keys"))
            .on_hover_text("Type without reading the keycaps");
        if before != s.save.target_wpm || was != (s.save.show_keyboard, s.save.show_fingers, s.save.hide_labels) {
            store(s);
        }
        if matches!(mode, Mode::Words | Mode::Sprint) {
            let tip = if s.has_shift {
                "Sentences with capitals, commas and full stops, the ones your keymap types"
            } else {
                "This keymap has no Shift on its base layer: commas and full stops only"
            };
            if ui.checkbox(&mut s.save.capitals, "Capitals & punctuation").on_hover_text(tip).changed() {
                s.new_line();
                store(s);
            }
        }
        if ui.button("Start over").clicked() {
            app.confirm = Some(Confirm {
                title: format!("Start {} over?", lang.name()),
                body: "The letters learned and their speeds are forgotten for this language.".into(),
                ok: "Start over".into(),
                action: Action::ResetPractice,
            });
        }
    });
}

/// Letter pairs typed by one finger: two different keys of the base layer under
/// the same finger of the same hand.
fn same_finger_pairs(
    app: &App,
    alphabet: &[char],
    places: &std::collections::BTreeMap<char, practice::Place>,
) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    let at: Vec<(char, practice::Place)> = alphabet
        .iter()
        .filter_map(|c| places.get(c).filter(|p| p.layer == 0).map(|p| (*c, *p)))
        .collect();
    for &(a, pa) in &at {
        for &(b, pb) in &at {
            let other_key = (pa.row, pa.col) != (pb.row, pb.col);
            if other_key && hand_and_finger(app, pa.row, pa.col) == hand_and_finger(app, pb.row, pb.col) {
                out.insert([a, b].iter().collect());
            }
        }
    }
    out
}

fn pairs_strip(s: &Session, ui: &mut egui::Ui) {
    let p = s.progress();
    let target = s.save.target_wpm;
    ui.horizontal_wrapped(|ui| {
        for pair in s.weak_pairs() {
            let st = p.pairs.get(&pair).copied().unwrap_or_default();
            let fill = if st.samples == 0 { theme::FG_SECONDARY } else { mastery(st.confidence(target)) };
            let one_finger = s.same_finger.contains(&pair);
            let stroke = if one_finger { Stroke::new(2.0, theme::ORANGE) } else { Stroke::NONE };
            let text = RichText::new(&pair).monospace().size(15.0).color(theme::DARK_TEXT);
            let tip = match st.wpm() {
                Some(w) => format!("{w:.0} wpm, {:.0}% right, {} times", (1.0 - st.errors) * 100.0, st.samples),
                None => "Not timed yet".to_string(),
            };
            let tip = if one_finger { format!("{tip}. One finger types both keys on this keymap.") } else { tip };
            ui.add(egui::Button::new(text).fill(fill).stroke(stroke).sense(Sense::hover())).on_hover_text(tip);
        }
    });
    note(ui, "The slowest letter pairs. Outlined in orange: one finger types both keys on your keymap.");
}

fn letters_strip(s: &Session, ui: &mut egui::Ui) {
    if s.mode() == Mode::Bigrams {
        pairs_strip(s, ui);
        return;
    }
    let p = s.progress();
    let letters = s.pool();
    let focus = s.focus();
    let target = s.save.target_wpm;
    let shown = if s.mode() == Mode::Symbols { &s.symbols } else { &s.alphabet };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for &c in shown {
            let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 26.0), Sense::hover());
            let st = p.stat(c);
            let tip = if !letters.contains(&c) {
                "Not in play yet".to_string()
            } else if st.samples == 0 {
                "Not typed yet".to_string()
            } else {
                let speed = st.wpm().map_or("—".to_string(), |w| format!("{w:.0} wpm"));
                let state = if st.known(target) { "known" } else { "learning" };
                format!("{speed}, {:.0}% right, {} presses, {state}", (1.0 - st.errors) * 100.0, st.samples)
            };
            resp.on_hover_text(tip);
            let in_play = letters.contains(&c);
            let fill = match (in_play, p.stat(c).samples) {
                (false, _) => theme::BG_SECONDARY,
                (true, 0) => theme::FG_SECONDARY,
                (true, _) => mastery(p.stat(c).confidence(target)),
            };
            let stroke = if focus == Some(c) { Stroke::new(2.0, theme::CYAN) } else { Stroke::NONE };
            ui.painter().rect(rect, 4.0, fill, stroke, egui::StrokeKind::Inside);
            let text = if in_play { theme::DARK_TEXT } else { theme::FG_MUTED };
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, c, FontId::monospace(14.0), text);
        }
    });
    let count = match s.mode() {
        Mode::Letters => format!("{} of {} letters, the next comes once these reach {target} wpm", letters.len(), s.alphabet.len()),
        Mode::Words => format!("Every letter, {} of them", letters.len()),
        Mode::Symbols => format!("{} signs, each shown with the layer key to hold", letters.len()),
        Mode::Bigrams => String::new(),
        Mode::Sprint => format!(
            "60 seconds of words against the clock, record {:.0} wpm. The clock starts with your first key.",
            s.save.game.best_sprint
        ),
    };
    note(ui, &count);
}

fn typing_line(s: &Session, ui: &mut egui::Ui) {
    let wrong = ui.input(|i| i.time) < s.wrong_until;
    if s.mode() == Mode::Sprint {
        let left = s.sprint_left(ui.input(|i| i.time)).unwrap_or(practice::SPRINT_SECONDS);
        let color = if left < 10.0 { theme::RED } else { theme::YELLOW };
        ui.label(RichText::new(format!("{:.0} s", left.ceil())).size(30.0).strong().color(color));
    }
    card(ui, |ui| {
        let font = FontId::monospace(26.0);
        let mut job = LayoutJob::default();
        for (i, &c) in s.text.iter().enumerate() {
            let mut f = TextFormat { font_id: font.clone(), ..Default::default() };
            if i < s.pos {
                f.color = theme::FG_MUTED;
            } else if i == s.pos {
                f.color = theme::DARK_TEXT;
                f.background = if wrong { theme::RED } else { theme::CYAN };
            } else {
                f.color = theme::FG_PRIMARY;
            }
            // A space under the cursor still needs to be seen.
            let shown = if c == ' ' && i == s.pos { '␣' } else { c };
            job.append(&shown.to_string(), 0.0, f);
        }
        job.wrap.max_width = ui.available_width();
        ui.label(job);
        let now = ui.input(|i| i.time);
        let speed = s.live_wpm(now).map_or("—".to_string(), |w| format!("{w:.0} wpm"));
        ui.horizontal(|ui| {
            note(ui, &format!("{} / {} characters, {speed}", s.pos, s.text.len()));
            if s.combo >= 10 {
                // The combo warms up from cyan to orange as it grows.
                let heat = (s.combo as f32 / 100.0).min(1.0);
                let color = theme::CYAN.lerp_to_gamma(theme::ORANGE, heat);
                let size = 13.0 + 5.0 * heat;
                ui.label(RichText::new(format!("combo {}", s.combo)).size(size).strong().color(color));
            }
        });
    });
}

fn hand_and_finger(app: &App, row: usize, col: usize) -> (stats::Hand, stats::Finger) {
    let km = app.doc.layer(0);
    let rows = km.map_or(0, Vec::len);
    let cols = km.and_then(|k| k.first()).map_or(0, Vec::len);
    let (hand, finger) = stats::hand_finger(row, col, rows, cols);
    // The KaSe rule gives whole centre columns to the thumbs; only the last row is theirs.
    if finger == stats::Finger::Thumb && row + 1 < rows {
        return (hand, stats::Finger::Index);
    }
    (hand, finger)
}

/// One colour per finger, the same on both hands.
fn finger_color(f: stats::Finger) -> Color32 {
    match f {
        stats::Finger::Pinky => theme::PURPLE,
        stats::Finger::Ring => theme::CYAN,
        stats::Finger::Middle => theme::GREEN,
        stats::Finger::Index => theme::ORANGE,
        stats::Finger::Thumb => theme::YELLOW,
    }
}

fn finger_of(app: &App, row: usize, col: usize) -> String {
    let (hand, finger) = hand_and_finger(app, row, col);
    let hand = if hand == stats::Hand::Left { "left" } else { "right" };
    format!("{hand} {}", stats::FINGER_NAMES[finger as usize].to_lowercase())
}

fn next_key_hint(
    app: &App,
    s: &Session,
    places: &std::collections::BTreeMap<char, practice::Place>,
    shift: Option<(usize, usize)>,
    ui: &mut egui::Ui,
) {
    let Some(c) = s.expected() else { return };
    let Some(p) = places.get(&practice::lower(c)) else {
        note(ui, "Next: a character this keymap doesn't have.");
        return;
    };
    let shown = if c == ' ' { "space".to_string() } else { c.to_string() };
    let mut text = format!("Next: {shown}, {}", finger_of(app, p.row, p.col));
    if let Some((r, col)) = p.hold {
        let layer = app.layer_names.get(p.layer).cloned().unwrap_or_else(|| format!("layer {}", p.layer));
        let holder = app.doc.get((0, r, col)).map(|code| keys::label_for(code, &app.kb_layout)).unwrap_or_default();
        text.push_str(&format!(", on {layer}: hold {holder} with the {}", finger_of(app, r, col)));
    }
    if let (true, Some((r, col))) = (c.is_uppercase(), shift) {
        let label = app.doc.get((0, r, col)).map(|code| keys::label_for(code, &app.kb_layout)).unwrap_or_default();
        text.push_str(&format!(", a capital: hold {label} with the {}", finger_of(app, r, col)));
    }
    ui.label(RichText::new(text).size(13.0).color(theme::CYAN));
}

fn keyboard(
    app: &App,
    s: &Session,
    places: &std::collections::BTreeMap<char, practice::Place>,
    shift: Option<(usize, usize)>,
    ui: &mut egui::Ui,
) {
    let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), 260.0), Sense::hover());
    ui.painter().rect_filled(area, 8.0, theme::BG_SURFACE);
    let painter = ui.painter_at(area);
    let view = View::fit(&app.keys, area.shrink2(vec2(8.0, 14.0)));
    let next = s.expected().and_then(|c| places.get(&practice::lower(c))).copied();
    let capital = s.expected().is_some_and(char::is_uppercase);
    let layer = next.map_or(0, |p| p.layer);
    let letters = s.pool();
    let p = s.progress();
    let now = ui.input(|i| i.time);
    let wrong = s.wrong_char.filter(|_| now < s.wrong_until).and_then(|c| places.get(&practice::lower(c))).map(|w| (w.row, w.col));
    let blank = s.save.hide_labels;
    for k in &app.keys {
        let code = app.doc.get((layer, k.row, k.col));
        // An empty key on that layer falls through: no "None" on it.
        let label = match code {
            Some(c) if c != 0 && !blank => keys::label_for(c, &app.kb_layout),
            _ => String::new(),
        };
        let typed = code.and_then(|c| practice::char_of(c, &app.kb_layout));
        let here = (k.row, k.col);
        let (fill, text) = if wrong == Some(here) {
            (theme::RED, theme::DARK_TEXT)
        } else if next.is_some_and(|n| (n.row, n.col) == here) {
            (theme::CYAN, theme::DARK_TEXT)
        } else if next.and_then(|n| n.hold) == Some(here) {
            (theme::PURPLE, theme::DARK_TEXT)
        } else if let Some(c) = typed.filter(|c| letters.contains(c) && p.stat(*c).samples > 0) {
            (mastery(p.stat(c).confidence(s.save.target_wpm)).gamma_multiply(0.55), theme::FG_PRIMARY)
        } else {
            (theme::KEYCAP.gamma_multiply(0.6), theme::FG_SECONDARY)
        };
        let stroke = if s.save.show_fingers {
            Stroke::new(2.5, finger_color(hand_and_finger(app, k.row, k.col).1))
        } else {
            Stroke::new(1.0, theme::BG_PRIMARY)
        };
        let style = CapStyle { fill, stroke, text };
        paint_key(&painter, &view, k, &style, &label);
    }
    // The hold key lives on the base layer: show its own label there.
    // The layer key to hold, and Shift for a capital: both from the base layer.
    let holds = [next.and_then(|n| n.hold), shift.filter(|_| capital)];
    for (r, c) in holds.into_iter().flatten() {
        if let Some(k) = app.keys.iter().find(|k| (k.row, k.col) == (r, c)) {
            let label = if blank { String::new() } else { app.doc.get((0, r, c)).map(|code| keys::label_for(code, &app.kb_layout)).unwrap_or_default() };
            let style = CapStyle { fill: theme::PURPLE, stroke: Stroke::new(1.0, theme::BG_PRIMARY), text: theme::DARK_TEXT };
            paint_key(&painter, &view, k, &style, &label);
        }
    }
    if s.save.show_fingers {
        ui.horizontal(|ui| {
            for (name, f) in [
                ("Pinky", stats::Finger::Pinky),
                ("Ring", stats::Finger::Ring),
                ("Middle", stats::Finger::Middle),
                ("Index", stats::Finger::Index),
                ("Thumb", stats::Finger::Thumb),
            ] {
                let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                ui.painter().rect_stroke(r, 2.0, Stroke::new(2.5, finger_color(f)), egui::StrokeKind::Inside);
                note(ui, name);
            }
        });
    }
}

/// A five-pointed star: a pentagon and five triangles, all convex.
fn paint_star(painter: &egui::Painter, center: egui::Pos2, r: f32, fill: Color32) {
    let point = |i: usize, radius: f32| {
        let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
        center + vec2(a.cos(), a.sin()) * radius
    };
    let inner: Vec<egui::Pos2> = (0..5).map(|i| point(2 * i + 1, r * 0.42)).collect();
    painter.add(egui::Shape::convex_polygon(inner.clone(), fill, Stroke::NONE));
    for i in 0..5 {
        let tip = point(2 * i, r);
        let left = inner[(i + 4) % 5];
        painter.add(egui::Shape::convex_polygon(vec![tip, inner[i], left], fill, Stroke::NONE));
    }
}

fn bar(ui: &mut egui::Ui, width: f32, share: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 8.0), Sense::hover());
    ui.painter().rect_filled(rect, 4.0, theme::BG_PRIMARY);
    let mut done = rect;
    done.set_width(rect.width() * share.clamp(0.0, 1.0));
    ui.painter().rect_filled(done, 4.0, color);
}

fn game_banner(s: &Session, ui: &mut egui::Ui) {
    let g = &s.save.game;
    let (level, into) = g.level();
    let needed = game::xp_for_level(level);
    let today = g.lines_on(s.today);
    let streak = g.streak_on(s.today);
    card(ui, |ui| {
        ui.columns(3, |c| {
            c[0].label(RichText::new(format!("Level {level}")).size(18.0).strong().color(theme::PURPLE));
            bar(&mut c[0], 180.0, into as f32 / needed as f32, theme::PURPLE);
            note(&mut c[0], &format!("{into} / {needed} XP"));

            c[1].label(RichText::new(format!("Today {today} / {DAILY_GOAL}")).size(18.0).strong().color(theme::GREEN));
            bar(&mut c[1], 180.0, today as f32 / DAILY_GOAL as f32, theme::GREEN);
            note(&mut c[1], if today >= DAILY_GOAL { "Daily goal met" } else { "lines for the daily goal" });

            let days = if streak == 1 { "day".to_string() } else { "days".to_string() };
            c[2].label(RichText::new(format!("{streak} {days} in a row")).size(18.0).strong().color(theme::ORANGE));
            note(&mut c[2], &format!("Best combo {}, {} lines in all", g.best_combo, g.total_lines));
        });
    });
}

fn results(s: &Session, ui: &mut egui::Ui) {
    let now = ui.input(|i| i.time);
    if let Some(r) = &s.last {
        card(ui, |ui| {
            ui.horizontal(|ui| {
                // The stars light up one after the other.
                let lit = (((now - r.at) / 0.18).max(0.0) as usize).min(r.reward.stars as usize);
                let (area, _) = ui.allocate_exact_size(vec2(5.0 * 34.0, 34.0), Sense::hover());
                for i in 0..5 {
                    let c = egui::pos2(area.left() + 17.0 + i as f32 * 34.0, area.center().y);
                    let fill = if i < lit { theme::YELLOW } else { theme::BG_PRIMARY };
                    paint_star(ui.painter(), c, 15.0, fill);
                }
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(format!("{:.0} wpm, {:.0}% right", r.wpm, r.accuracy * 100.0)).size(16.0),
                    );
                    ui.label(RichText::new(format!("+{} XP", r.reward.xp)).size(14.0).color(theme::PURPLE));
                    if s.mode() == Mode::Sprint {
                        note(ui, &format!("Sprint over, record {:.0} wpm", s.save.game.best_sprint));
                    }
                });
            });
            if let Some(level) = r.reward.level_up {
                ui.label(RichText::new(format!("Level {level}!")).size(18.0).strong().color(theme::PURPLE));
            }
            if let Some(c) = r.new_letter {
                ui.label(RichText::new(format!("New letter: {c}")).size(15.0).color(theme::GREEN));
            }
            for id in &r.reward.new_badges {
                if let Some(b) = game::badge(id) {
                    ui.label(RichText::new(format!("Badge earned: {}, {}", b.name, b.about.to_lowercase())).color(theme::ORANGE));
                }
            }
            if now - r.at < 1.2 {
                ui.ctx().request_repaint();
            }
        });
    }
    let p = s.progress();
    speed_chart(&p.history, s.save.target_wpm, ui);
    slips(&p, ui);
    ui.columns(2, |c| {
        let best = if p.lines == 0 { "—".to_string() } else { format!("{:.0}", p.best_wpm) };
        tile(&mut c[0], "Best", &best, "words per minute in this mode");
        tile(&mut c[1], "Lines", &p.lines.to_string(), "typed in this mode");
    });
    badges(s, ui);
}

/// The keys most often typed in place of another.
fn slips(p: &practice::LangProgress, ui: &mut egui::Ui) {
    let mut list: Vec<(&String, &u32)> = p.slips.iter().filter(|(_, n)| **n >= 2).collect();
    if list.is_empty() {
        return;
    }
    list.sort_by(|a, b| b.1.cmp(a.1));
    let shown = |c: char| if c == ' ' { "space".to_string() } else { c.to_string() };
    let text: Vec<String> = list
        .iter()
        .take(5)
        .filter_map(|(k, n)| {
            let mut ch = k.chars();
            Some(format!("{} → {} ×{n}", shown(ch.next()?), shown(ch.next()?)))
        })
        .collect();
    ui.label(RichText::new("Slips").size(13.0).color(theme::CYAN));
    note(ui, &format!("Wanted, then typed instead: {}", text.join(", ")));
}

/// Speed of the latest lines, with the target as a dashed line.
fn speed_chart(history: &[f32], target: u32, ui: &mut egui::Ui) {
    if history.len() < 2 {
        return;
    }
    ui.label(RichText::new(format!("Speed, last {} lines", history.len())).size(13.0).color(theme::CYAN));
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 90.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, theme::BG_SURFACE);
    let inner = rect.shrink2(vec2(10.0, 10.0));
    let top = history.iter().copied().fold(target as f32, f32::max) * 1.1;
    let at = |i: usize, w: f32| {
        let x = inner.left() + inner.width() * i as f32 / (history.len() - 1) as f32;
        egui::pos2(x, inner.bottom() - inner.height() * (w / top).clamp(0.0, 1.0))
    };
    let y = inner.bottom() - inner.height() * target as f32 / top;
    painter.extend(egui::Shape::dashed_line(
        &[egui::pos2(inner.left(), y), egui::pos2(inner.right(), y)],
        Stroke::new(1.0, theme::FG_MUTED),
        6.0,
        4.0,
    ));
    let points: Vec<egui::Pos2> = history.iter().enumerate().map(|(i, &w)| at(i, w)).collect();
    painter.add(egui::Shape::line(points.clone(), Stroke::new(2.0, theme::GREEN)));
    for (p, &w) in points.iter().zip(history) {
        let color = if w >= target as f32 { theme::GREEN } else { theme::ORANGE };
        painter.circle_filled(*p, 3.0, color);
    }
    let last = history.last().copied().unwrap_or(0.0);
    resp.on_hover_text(format!("Latest {last:.0} wpm, target {target} wpm"));
}

fn badges(s: &Session, ui: &mut egui::Ui) {
    let earned = &s.save.game.badges;
    ui.label(RichText::new(format!("Badges {} / {}", earned.len(), BADGES.len())).size(13.0).color(theme::CYAN));
    ui.horizontal_wrapped(|ui| {
        for b in &BADGES {
            let got = earned.contains(b.id);
            let (fill, text) = if got { (theme::ORANGE, theme::DARK_TEXT) } else { (theme::BG_SECONDARY, theme::FG_MUTED) };
            let label = RichText::new(b.name).size(12.0).color(text);
            ui.add(egui::Button::new(label).fill(fill).sense(Sense::hover())).on_hover_text(b.about);
        }
    });
}
