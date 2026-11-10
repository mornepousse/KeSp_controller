//! Drives the real window logic with simulated mouse and keyboard, no display
//! and no keyboard attached.

use crate::app::{App, Options, Tab};
use egui::{pos2, vec2, Key, Pos2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

fn harness(tab: Tab) -> Harness<'static, App> {
    let app = App::new(Options { auto_connect: false, tab, ..Default::default() });
    let mut h = Harness::builder()
        .with_size(vec2(1000.0, 800.0))
        .with_step_dt(0.05)
        .allow_missing_glyphs() // the test harness has no system fonts to fall back on
        .with_accessibility_check(false)
        .build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run_steps(4);
    h
}

fn key_center(h: &Harness<'_, App>, row: usize, col: usize) -> Pos2 {
    let app = h.state();
    let view = app.kb_view.expect("keyboard drawn");
    let k = app.keys.iter().find(|k| k.row == row && k.col == col).expect("key exists");
    view.key_rect(k).center()
}

fn click(h: &mut Harness<'_, App>, p: Pos2) {
    h.hover_at(p);
    h.run_steps(2);
    h.drag_at(p);
    h.run_steps(1);
    h.drop_at(p);
    h.run_steps(3);
}

fn drag(h: &mut Harness<'_, App>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.run_steps(2);
    h.drag_at(from);
    h.run_steps(2);
    let mid = from + (to - from) * 0.5;
    h.hover_at(mid);
    h.run_steps(2);
    h.hover_at(to);
    h.run_steps(2);
    h.drop_at(to);
    h.run_steps(3);
}

fn code_at(h: &Harness<'_, App>, row: usize, col: usize) -> Option<u16> {
    let app = h.state();
    app.doc.get((app.layer, row, col))
}

fn palette_tile(h: &Harness<'_, App>, label: &str) -> Pos2 {
    h.get_by_label(label).rect().center()
}

#[test]
fn on_a_small_window_the_keyboard_keeps_room_next_to_the_palette() {
    let app = App::new(Options { auto_connect: false, tab: Tab::Keymap, ..Default::default() });
    let mut h = Harness::builder()
        .with_size(vec2(915.0, 547.0))
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run_steps(4);
    let a = h.state();
    let top = a.keys.iter().map(|k| a.kb_view.unwrap().key_rect(k).top()).fold(f32::MAX, f32::min);
    let bottom = a.keys.iter().map(|k| a.kb_view.unwrap().key_rect(k).bottom()).fold(0.0, f32::max);
    assert!(bottom - top > 180.0, "keyboard only {} points tall", bottom - top);
}

#[test]
fn clicking_a_key_then_a_palette_tile_assigns_it_without_touching_a_keyboard() {
    let mut h = harness(Tab::Keymap);
    let p = key_center(&h, 2, 3);
    click(&mut h, p);
    assert!(h.state().selected.is_some(), "the click selects the key");
    let tile = palette_tile(&h, "Z");
    click(&mut h, tile);
    assert_eq!(code_at(&h, 2, 3), Some(0x1D));
    assert!(h.state().doc.is_pending((0, 2, 3)), "no keyboard: it waits as unsent");
}

#[test]
fn dragging_from_the_palette_drops_onto_the_key_under_the_pointer() {
    let mut h = harness(Tab::Keymap);
    let target = key_center(&h, 1, 8);
    let tile = palette_tile(&h, "Q");
    drag(&mut h, tile, target);
    assert_eq!(code_at(&h, 1, 8), Some(0x14));
    assert!(h.state().drag.is_none());
}

#[test]
fn dropping_off_the_keyboard_changes_nothing() {
    let mut h = harness(Tab::Keymap);
    let tile = palette_tile(&h, "Q");
    let before = h.state().doc.pending_count();
    drag(&mut h, tile, pos2(50.0, 400.0));
    assert_eq!(h.state().doc.pending_count(), before);
    assert!(h.state().drag.is_none());
}

#[test]
fn dragging_a_key_onto_another_swaps_them_and_undo_restores_both() {
    let mut h = harness(Tab::Keymap);
    let (a, b) = (key_center(&h, 2, 1), key_center(&h, 2, 11));
    let (va, vb) = (code_at(&h, 2, 1), code_at(&h, 2, 11));
    assert_ne!(va, vb);
    drag(&mut h, a, b);
    assert_eq!(code_at(&h, 2, 1), vb);
    assert_eq!(code_at(&h, 2, 11), va);
    h.state_mut().undo();
    assert_eq!(code_at(&h, 2, 1), va);
    assert_eq!(code_at(&h, 2, 11), vb);
}

#[test]
fn double_click_opens_the_selector_for_that_key() {
    let mut h = harness(Tab::Keymap);
    let p = key_center(&h, 0, 2);
    h.hover_at(p);
    h.run_steps(2);
    for _ in 0..2 {
        h.drag_at(p);
        h.run_steps(1);
        h.drop_at(p);
        h.run_steps(1);
    }
    h.run_steps(2);
    assert!(h.state().selector.is_some());
}

#[test]
fn arrows_walk_the_keyboard_and_delete_clears_the_key() {
    let mut h = harness(Tab::Keymap);
    let p = key_center(&h, 2, 1);
    click(&mut h, p);
    let start = h.state().selected.unwrap();
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    let moved = h.state().selected.unwrap();
    assert_ne!(moved, start);
    let (row, col) = {
        let k = &h.state().keys[moved];
        (k.row, k.col)
    };
    h.key_press(Key::Delete);
    h.run_steps(2);
    assert_eq!(code_at(&h, row, col), Some(0));
}

#[test]
fn type_to_assign_uses_the_pressed_key_and_moves_on() {
    let mut h = harness(Tab::Keymap);
    let p = key_center(&h, 2, 1);
    click(&mut h, p);
    let (row, col) = {
        let a = h.state();
        let k = &a.keys[a.selected.unwrap()];
        (k.row, k.col)
    };
    h.state_mut().capture = true;
    h.run_steps(1);
    h.key_press(Key::J);
    h.run_steps(2);
    assert_eq!(code_at(&h, row, col), Some(0x0D));
    let next = &h.state().keys[h.state().selected.unwrap()];
    assert_ne!((next.row, next.col), (row, col), "the selection moves on to the next key");
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(!h.state().capture, "Esc stops assigning");
}

#[test]
fn the_factory_layer_button_brings_the_firmware_keys_back() {
    let mut h = harness(Tab::Keymap);
    let original = code_at(&h, 2, 3);
    let p = key_center(&h, 2, 3);
    click(&mut h, p);
    let z = palette_tile(&h, "Z");
    click(&mut h, z);
    assert_ne!(code_at(&h, 2, 3), original);
    h.state_mut().restore_factory_layer();
    assert_eq!(code_at(&h, 2, 3), original);
}

#[test]
fn choosing_another_model_redraws_it_with_its_own_factory_keymap() {
    let mut h = harness(Tab::Keymap);
    let idx = h.state().boards.iter().position(|b| b.id == "niphargus").unwrap();
    h.state_mut().show_board(idx);
    h.run_steps(2);
    let a = h.state();
    assert_eq!(a.keys.len(), 52);
    assert_eq!(a.layer_names[0], "MAIN");
    // K_Y sits on column 7 of the Niphargus' first row.
    assert_eq!(code_at(&h, 0, 7), Some(0x1C));
}

#[test]
fn a_key_dragged_on_the_layout_tab_lands_on_the_grid() {
    let mut h = harness(Tab::Layout);
    let before = h.state().layout_ed.keys[0].clone();
    let tile = find_layout_key(&mut h, before.row, before.col);
    drag(&mut h, tile, tile + vec2(120.0, 40.0));
    let after = h.state().layout_ed.keys[0].clone();
    assert!(after.x > before.x, "moved right: {} -> {}", before.x, after.x);
    let step = 0.25 * crate::protocol::layout::UNIT_PX;
    assert!((after.y / step).fract().abs() < 0.001, "on the 0.25u grid: {}", after.y);
    assert!(h.state().layout_ed.dirty);
}

/// The Layout tab paints its keys, so look for one by hovering until the tooltip
/// free hit-test selects it: scan the canvas with clicks on a coarse grid.
fn find_layout_key(h: &mut Harness<'_, App>, row: usize, col: usize) -> Pos2 {
    for y in (150..760).step_by(14) {
        for x in (20..780).step_by(14) {
            let p = pos2(x as f32, y as f32);
            click(h, p);
            let ed = &h.state().layout_ed;
            if let [one] = ed.selection[..] {
                if ed.keys[one].row == row && ed.keys[one].col == col {
                    return p;
                }
            }
        }
    }
    panic!("key R{row}C{col} not found on the canvas");
}

fn demo_harness(tab: Tab) -> Harness<'static, App> {
    let app = App::new(Options { auto_connect: false, demo: true, tab, ..Default::default() });
    let mut h = Harness::builder()
        .with_size(vec2(1000.0, 800.0))
        .with_step_dt(0.05)
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run_steps(3);
    h
}

#[test]
fn every_page_of_every_tab_draws_with_data() {
    use crate::app::{Behavior, DevicePage};
    for tab in Tab::ALL {
        let mut h = demo_harness(tab);
        for b in [Behavior::Macros, Behavior::TapDance, Behavior::Combos, Behavior::Overrides, Behavior::Leader, Behavior::Layers] {
            h.state_mut().behavior = b;
            h.run_steps(2);
        }
        for d in [DevicePage::Status, DevicePage::Backup, DevicePage::Firmware, DevicePage::MatrixTest, DevicePage::Reset] {
            h.state_mut().device_page = d;
            h.run_steps(2);
        }
    }
}

#[test]
fn the_radio_card_shows_each_half_from_its_own_figures() {
    let mut h = demo_harness(Tab::Device);
    h.state_mut().is_dongle = true;
    h.run_steps(2);
    assert!(h.query_by_label_contains("1204 frames, 4% retries").is_some());
    assert!(h.query_by_label_contains("628 frames, 35% retries").is_some());
    assert!(h.query_by_label_contains("heard 12 s ago").is_some());
}

#[test]
fn the_radio_card_says_so_when_the_firmware_has_no_figures_per_half() {
    let mut h = demo_harness(Tab::Device);
    h.state_mut().is_dongle = true;
    h.state_mut().dongle.rf.halves = None;
    h.run_steps(2);
    assert!(h.query_by_label_contains("frames,").is_none());
    assert!(h.query_by_label_contains("doesn't report each half").is_some());
}

#[test]
fn a_keyboard_at_rest_reads_idle_while_a_half_was_just_heard() {
    let mut h = demo_harness(Tab::Device);
    h.state_mut().is_dongle = true;
    h.state_mut().dongle.rf.link_kbd = false;
    h.run_steps(2);
    assert!(h.query_by_label_contains("idle").is_some());
    assert!(h.query_by_label_contains("not linked").is_none());
}

#[test]
fn practice_moves_on_with_the_right_key_only() {
    let mut h = demo_harness(Tab::Practice);
    h.run_steps(2);
    let want = h.state().practice.as_ref().and_then(|s| s.expected()).expect("a line to type");
    let wrong = if want == 'z' { 'q' } else { 'z' };
    h.event(egui::Event::Text(wrong.to_string()));
    h.run_steps(2);
    assert_eq!(h.state().practice.as_ref().unwrap().pos, 0);
    h.event(egui::Event::Text(want.to_string()));
    h.run_steps(2);
    assert_eq!(h.state().practice.as_ref().unwrap().pos, 1);
    assert!(h.query_by_label_contains("Next:").is_some());
}

#[test]
fn a_finished_practice_line_shows_its_reward() {
    let mut h = demo_harness(Tab::Practice);
    h.run_steps(2);
    // The reward is drawn below the keyboard, out of this small window otherwise.
    h.state_mut().practice.as_mut().unwrap().save.show_keyboard = false;
    let text: String = h.state().practice.as_ref().unwrap().text.iter().collect();
    for c in text.chars() {
        h.event(egui::Event::Text(c.to_string()));
        h.run_steps(1);
    }
    h.run_steps(2);
    let s = h.state().practice.as_ref().unwrap();
    let xp = s.last.as_ref().expect("the line was finished").reward.xp;
    assert!(s.save.game.total_lines >= 1);
    assert!(h.query_by_label(&format!("+{xp} XP")).is_some());
    assert!(h.query_by_label_contains("Badge earned: First line").is_some());
}

#[test]
fn the_page_list_on_the_side_switches_page() {
    use crate::app::Behavior;
    let mut h = demo_harness(Tab::Behaviors);
    let row = pos2(60.0, 48.0 + 34.0 + 8.0 + 32.0 + 4.0 + 16.0);
    click(&mut h, row);
    assert_eq!(h.state().behavior, Behavior::TapDance);
}

#[test]
fn clicking_two_keys_on_the_combo_page_chooses_them() {
    use crate::app::Behavior;
    let mut h = demo_harness(Tab::Behaviors);
    h.state_mut().behavior = Behavior::Combos;
    h.run_steps(3);
    let centre = |h: &Harness<'_, App>, row: usize, col: usize| {
        let a = h.state();
        let k = a.keys.iter().find(|k| k.row == row && k.col == col).unwrap();
        a.combo_view.expect("combo keyboard drawn").key_rect(k).center()
    };
    let (p, q) = (centre(&h, 2, 1), centre(&h, 2, 11));
    click(&mut h, p);
    assert_eq!(h.state().adv.combo_keys, [Some((2, 1)), None]);
    click(&mut h, q);
    assert_eq!(h.state().adv.combo_keys, [Some((2, 1)), Some((2, 11))]);
}

#[test]
fn clicking_a_finding_goes_to_the_key_it_is_about() {
    let mut h = demo_harness(Tab::Keymap);
    h.state_mut().doc.set_layer(1, vec![vec![0u16; 13]; 5]);
    h.state_mut().macros_loaded = true;
    // M9 has no macro in the demo data.
    let keys = h.state().keys.clone();
    let k = keys.iter().find(|k| k.row == 2 && k.col == 3).unwrap().clone();
    h.state_mut().doc.apply(&[((0, k.row, k.col), 0x1E00)], false);
    h.state_mut().check_open = true;
    h.run_steps(3);
    let finding = h.state().findings().into_iter().find(|f| f.text.contains("M9")).expect("reported");
    assert_eq!(finding.at, Some((0, 2, 3)));
    let (l, r, c) = finding.at.unwrap();
    h.state_mut().jump_to(l, Some((r, c)));
    let a = h.state();
    let s = &a.keys[a.selected.unwrap()];
    assert_eq!((a.layer, s.row, s.col), (0, 2, 3));
}

#[test]
fn a_read_that_an_edit_overtook_does_not_undo_the_edit() {
    use crate::backend::BgMsg;
    let mut h = harness(Tab::Keymap);
    h.state_mut().conn = crate::app::Conn::Connected;
    h.state_mut().doc.set_layer(0, vec![vec![4u16; 13]; 5]);
    h.state_mut().selected = Some(0);
    let k = h.state().keys[0].clone();
    let old_edit = 0u64;
    h.state_mut().assign_key(0, 0x2C);
    // The read began before the edit: the old content must not come back.
    h.state_mut().be.tx.send(BgMsg::Keymap(0, vec![vec![4u16; 13]; 5], Some(old_edit)));
    h.state_mut().conn = crate::app::Conn::Disconnected;
    h.run_steps(2);
    assert_eq!(h.state().doc.get((0, k.row, k.col)), Some(0x2C));
}
