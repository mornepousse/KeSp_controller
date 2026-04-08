use crate::context::BgMsg;
use crate::protocol;
use crate::protocol::serial::SerialManager;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

/// Export all keyboard config to JSON file via rfd save dialog.
pub fn export_config(
    serial: &Arc<Mutex<SerialManager>>,
    tx: &mpsc::Sender<BgMsg>,
) -> Result<String, String> {
    use protocol::binary::cmd;
    use protocol::config_io::*;

    let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());

    let _ = tx.send(BgMsg::ConfigProgress(0.05, "Reading layer names...".into()));
    let layer_names = ser.get_layer_names().unwrap_or_default();
    let num_layers = layer_names.len().max(1);

    let mut keymaps = Vec::new();
    for layer in 0..num_layers {
        let progress = 0.05 + (layer as f32 / num_layers as f32) * 0.30;
        let _ = tx.send(BgMsg::ConfigProgress(progress, format!("Reading layer {}...", layer)));
        let km = ser.get_keymap(layer as u8).unwrap_or_default();
        keymaps.push(km);
    }

    let _ = tx.send(BgMsg::ConfigProgress(0.40, "Reading tap dances...".into()));
    let tap_dances = match ser.send_binary(cmd::TD_LIST, &[]) {
        Ok(resp) => {
            let td_raw = protocol::parsers::parse_td_binary(&resp.payload);
            td_raw.iter().enumerate()
                .filter(|(_, actions)| actions.iter().any(|&a| a != 0))
                .map(|(i, actions)| TdConfig { index: i as u8, actions: *actions })
                .collect()
        }
        Err(_) => Vec::new(),
    };

    let _ = tx.send(BgMsg::ConfigProgress(0.50, "Reading combos...".into()));
    let combos = match ser.send_binary(cmd::COMBO_LIST, &[]) {
        Ok(resp) => {
            protocol::parsers::parse_combo_binary(&resp.payload).iter().map(|c| ComboConfig {
                index: c.index, r1: c.r1, c1: c.c1, r2: c.r2, c2: c.c2, result: c.result,
            }).collect()
        }
        Err(_) => Vec::new(),
    };

    let _ = tx.send(BgMsg::ConfigProgress(0.60, "Reading key overrides...".into()));
    let key_overrides = match ser.send_binary(cmd::KO_LIST, &[]) {
        Ok(resp) => {
            protocol::parsers::parse_ko_binary(&resp.payload).iter().map(|ko| KoConfig {
                trigger_key: ko[0], trigger_mod: ko[1], result_key: ko[2], result_mod: ko[3],
            }).collect()
        }
        Err(_) => Vec::new(),
    };

    let _ = tx.send(BgMsg::ConfigProgress(0.70, "Reading leaders...".into()));
    let leaders = match ser.send_binary(cmd::LEADER_LIST, &[]) {
        Ok(resp) => {
            protocol::parsers::parse_leader_binary(&resp.payload).iter().map(|l| LeaderConfig {
                index: l.index, sequence: l.sequence.clone(), result: l.result, result_mod: l.result_mod,
            }).collect()
        }
        Err(_) => Vec::new(),
    };

    let _ = tx.send(BgMsg::ConfigProgress(0.80, "Reading macros...".into()));
    let macros = match ser.send_binary(cmd::LIST_MACROS, &[]) {
        Ok(resp) => {
            protocol::parsers::parse_macros_binary(&resp.payload).iter().map(|m| {
                let steps_str: Vec<String> = m.steps.iter()
                    .map(|s| format!("{:02X}:{:02X}", s.keycode, s.modifier))
                    .collect();
                MacroConfig { slot: m.slot, name: m.name.clone(), steps: steps_str.join(",") }
            }).collect()
        }
        Err(_) => Vec::new(),
    };

    drop(ser);

    let _ = tx.send(BgMsg::ConfigProgress(0.90, "Saving file...".into()));

    let config = KeyboardConfig {
        version: 1,
        layer_names,
        keymaps,
        tap_dances,
        combos,
        key_overrides,
        leaders,
        macros,
    };

    let json = config.to_json()?;

    let file = rfd::FileDialog::new()
        .add_filter("KeSp Config", &["json"])
        .set_file_name("kesp_config.json")
        .save_file();

    match file {
        Some(path) => {
            std::fs::write(&path, &json).map_err(|e| format!("Write error: {}", e))?;
            Ok(format!("Exported to {}", path.display()))
        }
        None => Ok("Export cancelled".into()),
    }
}

/// Import keyboard config using binary protocol v2.
pub fn import_config(
    serial: &Arc<Mutex<SerialManager>>,
    tx: &mpsc::Sender<BgMsg>,
    config: &protocol::config_io::KeyboardConfig,
) -> Result<String, String> {
    use protocol::binary as bp;

    let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
    let mut errors = 0usize;

    let total_steps = (config.layer_names.len()
        + config.keymaps.len()
        + config.tap_dances.len()
        + config.combos.len()
        + config.key_overrides.len()
        + config.leaders.len()
        + config.macros.len())
        .max(1) as f32;
    let mut done = 0usize;

    let _ = tx.send(BgMsg::ConfigProgress(0.0, "Setting layer names...".into()));
    for (i, name) in config.layer_names.iter().enumerate() {
        let payload = bp::set_layout_name_payload(i as u8, name);
        if ser.send_binary(bp::cmd::SET_LAYOUT_NAME, &payload).is_err() { errors += 1; }
        done += 1;
    }

    for (layer, km) in config.keymaps.iter().enumerate() {
        let progress = done as f32 / total_steps;
        let _ = tx.send(BgMsg::ConfigProgress(progress, format!("Writing layer {}...", layer)));
        let payload = bp::setlayer_payload(layer as u8, km);
        if let Err(e) = ser.send_binary(bp::cmd::SETLAYER, &payload) {
            eprintln!("SETLAYER {} failed: {}", layer, e);
            errors += 1;
        }
        done += 1;
    }

    let _ = tx.send(BgMsg::ConfigProgress(done as f32 / total_steps, "Setting tap dances...".into()));
    for td in &config.tap_dances {
        let payload = bp::td_set_payload(td.index, &td.actions);
        if ser.send_binary(bp::cmd::TD_SET, &payload).is_err() { errors += 1; }
        done += 1;
    }

    let _ = tx.send(BgMsg::ConfigProgress(done as f32 / total_steps, "Setting combos...".into()));
    for combo in &config.combos {
        let payload = bp::combo_set_payload(combo.index, combo.r1, combo.c1, combo.r2, combo.c2, combo.result as u8);
        if ser.send_binary(bp::cmd::COMBO_SET, &payload).is_err() { errors += 1; }
        done += 1;
    }

    let _ = tx.send(BgMsg::ConfigProgress(done as f32 / total_steps, "Setting key overrides...".into()));
    for (i, ko) in config.key_overrides.iter().enumerate() {
        let payload = bp::ko_set_payload(i as u8, ko.trigger_key, ko.trigger_mod, ko.result_key, ko.result_mod);
        if ser.send_binary(bp::cmd::KO_SET, &payload).is_err() { errors += 1; }
        done += 1;
    }

    let _ = tx.send(BgMsg::ConfigProgress(done as f32 / total_steps, "Setting leaders...".into()));
    for leader in &config.leaders {
        let payload = bp::leader_set_payload(leader.index, &leader.sequence, leader.result, leader.result_mod);
        if ser.send_binary(bp::cmd::LEADER_SET, &payload).is_err() { errors += 1; }
        done += 1;
    }

    let _ = tx.send(BgMsg::ConfigProgress(done as f32 / total_steps, "Setting macros...".into()));
    for m in &config.macros {
        let payload = bp::macro_add_seq_payload(m.slot, &m.name, &m.steps);
        if ser.send_binary(bp::cmd::MACRO_ADD_SEQ, &payload).is_err() { errors += 1; }
    }

    let _ = tx.send(BgMsg::ConfigProgress(0.95, "Refreshing...".into()));
    let names = ser.get_layer_names().unwrap_or_default();
    let km = ser.get_keymap(0).unwrap_or_default();
    let _ = tx.send(BgMsg::LayerNames(names));
    let _ = tx.send(BgMsg::Keymap(km));

    let total_keys: usize = config.keymaps.iter()
        .map(|l| l.iter().map(|r| r.len()).sum::<usize>())
        .sum();

    if errors > 0 {
        Ok(format!("Import done with {} errors (check stderr)", errors))
    } else {
        Ok(format!("Imported: {} layers, {} keys, {} TD, {} combos, {} KO, {} leaders, {} macros",
            config.layer_names.len(),
            total_keys,
            config.tap_dances.len(),
            config.combos.len(),
            config.key_overrides.len(),
            config.leaders.len(),
            config.macros.len(),
        ))
    }
}
