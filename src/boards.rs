//! The keyboards of the firmware, embedded at build time: shape, layer names
//! and factory keymap. `tools/import_boards.py` makes these files from the
//! firmware's board folders.

use crate::protocol::device;
use crate::protocol::layout::{self, KeycapPos};
use serde::Deserialize;

#[derive(Deserialize)]
struct BoardFile {
    id: String,
    name: String,
    product: String,
    layout: serde_json::Value,
    layer_names: Vec<String>,
    keymaps: Vec<Vec<Vec<u16>>>,
}

#[derive(Clone)]
pub struct Board {
    pub id: String,
    pub name: String,
    pub product: String,
    #[allow(dead_code)]
    pub rows: usize,
    pub cols: usize,
    pub keys: Vec<KeycapPos>,
    pub layer_names: Vec<String>,
    pub keymaps: Vec<Vec<Vec<u16>>>,
}

const FILES: [&str; 4] = [
    include_str!("../boards/kase_v2.json"),
    include_str!("../boards/kase_v1.json"),
    include_str!("../boards/niphargus.json"),
    include_str!("../boards/niphargus_right.json"),
];

pub fn all() -> Vec<Board> {
    FILES
        .iter()
        .map(|text| {
            let f: BoardFile = serde_json::from_str(text).expect("embedded board file");
            let json = f.layout.to_string();
            let (rows, cols) = device::resolve_geometry(layout::parse_geometry(&json), None);
            Board {
                keys: layout::parse_json(&json).expect("embedded board layout"),
                id: f.id,
                name: f.name,
                product: f.product,
                rows,
                cols,
                layer_names: f.layer_names,
                keymaps: f.keymaps,
            }
        })
        .collect()
}

/// Model of the connected keyboard, from the name it reports. The debug build of
/// the V2 shares the V2's shape and keymap; a dongle relays a Niphargus.
pub fn index_for_device(boards: &[Board], device_name: &str) -> Option<usize> {
    let wanted = match device_name {
        "KaSe V2 Debug" => "KaSe V2",
        "KaSe Dongle" => "Niphargus Left",
        other => other,
    };
    boards.iter().position(|b| b.product == wanted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::keycode;
    use crate::shape::duplicate_positions;

    fn board(id: &str) -> Board {
        all().into_iter().find(|b| b.id == id).unwrap()
    }

    #[test]
    fn every_board_matches_the_firmware_geometry() {
        let b = board("kase_v2");
        assert_eq!((b.rows, b.cols, b.keys.len()), (5, 13, 64));
        let b = board("kase_v1");
        assert_eq!((b.rows, b.cols, b.keys.len()), (5, 13, 64));
        let b = board("niphargus");
        assert_eq!((b.rows, b.cols, b.keys.len()), (4, 14, 52));
        let b = board("niphargus_right");
        assert_eq!((b.rows, b.cols, b.keys.len()), (4, 7, 26));
    }

    #[test]
    fn keys_stay_inside_their_matrix_and_never_share_a_position() {
        for b in all() {
            assert!(duplicate_positions(&b.keys).is_empty(), "{}", b.id);
            for k in &b.keys {
                assert!(k.row < b.rows && k.col < b.cols, "{} R{}C{}", b.id, k.row, k.col);
            }
        }
    }

    #[test]
    fn keymaps_have_one_code_per_matrix_position() {
        for b in all() {
            assert!(!b.keymaps.is_empty(), "{}", b.id);
            for layer in &b.keymaps {
                assert_eq!(layer.len(), b.rows, "{}", b.id);
                assert!(layer.iter().all(|r| r.len() == b.cols), "{}", b.id);
            }
        }
    }

    #[test]
    fn factory_keymaps_read_like_the_firmware_source() {
        let v2 = board("kase_v2");
        let names = |row: &[u16]| row.iter().map(|&c| keycode::decode_keycode(c)).collect::<Vec<_>>();
        // {K_DEL, K_1, K_2, K_3, ...}
        assert_eq!(keycode::decode_keycode(v2.keymaps[0][0][0]), "Del");
        assert_eq!(names(&v2.keymaps[0][1])[1..3], ["Q".to_string(), "W".to_string()]);
        // MO_L1 on the V2's row 1, column 6.
        assert_eq!(v2.keymaps[0][1][6], 0x0200);
        // Niphargus right half: columns 7-13 carry the keymap, K_Y on column 7.
        let n = board("niphargus");
        assert_eq!(keycode::decode_keycode(n.keymaps[0][0][7]), "Y");
        // A {{XXXXXXX}} layer is all empty.
        assert!(n.keymaps[2].iter().flatten().all(|&c| c == 0));
    }

    #[test]
    fn layer_names_come_from_the_firmware() {
        let n = board("niphargus");
        assert_eq!(n.layer_names[0], "MAIN");
        assert_eq!(n.layer_names[1], "NAV");
        assert_eq!(n.layer_names.len(), 10);
    }

    #[test]
    fn connected_devices_find_their_model_by_reported_name() {
        let bs = all();
        let id = |name: &str| index_for_device(&bs, name).map(|i| bs[i].id.clone());
        assert_eq!(id("KaSe V2 Debug").as_deref(), Some("kase_v2"));
        assert_eq!(id("Niphargus Left").as_deref(), Some("niphargus"));
        assert_eq!(id("KaSe Dongle").as_deref(), Some("niphargus"));
        assert_eq!(id("Something Else"), None);
    }
}
