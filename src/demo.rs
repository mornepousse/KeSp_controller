//! Sample data for `--demo`: lets every tab show something with no keyboard.
//! It only fills the window's own state; nothing is ever sent anywhere.

use crate::app::App;
use crate::backend::TamaStats;
use crate::protocol::{parsers, stats};

const COMBO_SAMPLE: [(u8, u8, u8, u8, u8, u16); 2] = [(0, 2, 3, 2, 9, 0x29), (1, 1, 4, 1, 8, 0x2A)];

fn sample_keymap() -> Vec<Vec<u16>> {
    // Row 0-3 of the 5x13 sample, thumb row last. HID usages.
    let rows: [[u16; 13]; 5] = [
        [0x29, 0x1E, 0x1F, 0x20, 0x21, 0x22, 0x2B, 0x23, 0x24, 0x25, 0x26, 0x27, 0x2D],
        [0x2B, 0x14, 0x1A, 0x08, 0x15, 0x17, 0x0000, 0x1C, 0x18, 0x0C, 0x12, 0x13, 0x2F],
        [0xE0, 0x04, 0x16, 0x07, 0x09, 0x0A, 0x28, 0x0B, 0x0D, 0x0E, 0x0F, 0x33, 0x34],
        [0xE1, 0x1D, 0x1B, 0x06, 0x19, 0x05, 0x2A, 0x11, 0x10, 0x36, 0x37, 0x38, 0xE5],
        [0xE3, 0xE2, 0x0200, 0x4100 | 0x2C, 0x5100 | 0x28, 0x2C, 0x2A, 0x0300, 0x3200, 0x3C00, 0x3400, 0xE6, 0xE7],
    ];
    rows.iter().map(|r| r.to_vec()).collect()
}

fn sample_heat() -> (Vec<Vec<u32>>, u32) {
    let mut max = 0;
    let data: Vec<Vec<u32>> = (0..5)
        .map(|r| {
            (0..13_i32)
                .map(|c| {
                    let home = if r == 2 { 3 } else { 1 };
                    let center = 7 - (c - 6).abs();
                    let v = (center.max(1) as u32) * home * 410 + (r * 37 + c * 11) as u32 % 90;
                    max = max.max(v);
                    v
                })
                .collect()
        })
        .collect();
    (data, max)
}

pub fn load(app: &mut App) {
    app.status = "Demo data: nothing is sent to a keyboard".into();
    app.layer_names = ["Base", "Nav", "Symbols", "Fn"].map(String::from).to_vec();
    let base = sample_keymap();
    let mut nav = vec![vec![0u16; 13]; 5];
    nav[1][2] = 0x52;
    nav[2][1] = 0x50;
    nav[2][2] = 0x51;
    nav[2][3] = 0x4F;
    app.doc.forget_device();
    if let Some(i) = app.boards.iter().position(|b| b.id == "kase_v2") {
        app.board = i;
        app.keys = app.boards[i].keys.clone();
    }
    app.doc.set_layer(0, base);
    app.doc.set_layer(1, nav);
    app.firmware = "demo".into();
    app.mode_label = "demo".into();
    app.wpm = 62;

    let (heat, max) = sample_heat();
    app.apply_heat(heat, max);
    app.status = "Demo data: nothing is sent to a keyboard".into();
    let lines: Vec<String> = [
        "R2C3 -> R2C9 : 900",
        "R2C2 -> R2C3 : 420",
        "R1C3 -> R2C3 : 130",
    ]
    .map(String::from)
    .to_vec();
    app.stats.bigrams = Some(stats::analyze_bigrams(&stats::parse_bigram_lines(&lines), 5, 13));
    app.stats.tama = Some(TamaStats {
        enabled: true,
        level: 4,
        hunger: 620,
        happiness: 810,
        energy: 540,
        health: 930,
        xp: 900,
        total_keys: 182_450,
        max_kpm: 412,
    });

    app.adv.tap_dances[0] = [0x29, 0x2B, 0x39, 0xE0];
    app.adv.combos = COMBO_SAMPLE
        .iter()
        .map(|&(index, r1, c1, r2, c2, result)| parsers::ComboEntry { index, r1, c1, r2, c2, result })
        .collect();
    app.adv.key_overrides = vec![parsers::KoEntry {
        index: 0,
        trigger_key: 0x2A,
        trigger_mod: 0x02,
        result_key: 0x4C,
        result_mod: 0,
    }];
    app.adv.leaders = vec![parsers::LeaderEntry {
        index: 0,
        sequence: vec![0x16, 0x0C],
        result: 0x16,
        result_mod: 0x01,
    }];
    app.adv.bt_status = "Slot 0: paired\nSlot 1: empty\nSlot 2: empty".into();
    app.adv.tama_status = "Level 4, happy".into();
    app.adv.autoshift = "Autoshift: OFF".into();
    app.mac.steps = vec![(0xE0, 0), (0x06, 0), (0xFF, 5)];
    app.mac.name = "copy".into();
    app.macros = vec![parsers::MacroEntry {
        slot: 0,
        name: "sign".into(),
        keys: Vec::new(),
        steps: vec![
            parsers::MacroStep { keycode: 0x16, modifier: 0 },
            parsers::MacroStep { keycode: 0xFF, modifier: 10 },
            parsers::MacroStep { keycode: 0x28, modifier: 0 },
        ],
    }];
    app.mac.slot = 1;
    app.macros_loaded = true;
    app.tap_dances_loaded = true;
    app.dongle.mac_left = "AA:BB:CC:00:11:22".into();
    app.dongle.mac_right = "—".into();
    app.dongle.paired = 1;
    app.dongle.rf.link_left = true;
    app.dongle.rf.sig_left = 210;
    app.dongle.rf.hb_age_left_ms = 1000;
    app.dongle.rf.pkt_rx_left = 1832;
    app.dongle.rf.pkt_dup_left = 4;
}
