use serde::Deserialize;

/// A keycap with computed absolute position.
#[derive(Clone, Debug, PartialEq)]
pub struct KeycapPos {
    pub row: usize,
    pub col: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub angle: f32, // degrees
}

pub const UNIT_PX: f32 = 50.0; // 1u = 50 pixels

#[derive(Deserialize)]
struct LayoutJson {
    #[allow(dead_code)]
    name: Option<String>,
    #[allow(dead_code)]
    rows: Option<usize>,
    #[allow(dead_code)]
    cols: Option<usize>,
    #[serde(default)]
    keys: Vec<KeyJson>,
    #[serde(default)]
    groups: Vec<GroupJson>,
}

#[derive(Deserialize)]
struct GroupJson {
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
    #[serde(default)]
    r: f32,
    keys: Vec<KeyJson>,
}

#[derive(Deserialize)]
struct KeyJson {
    row: usize,
    col: usize,
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
    #[serde(default = "default_one")]
    w: f32,
    #[serde(default = "default_one")]
    h: f32,
    #[serde(default)]
    r: f32,
}

fn default_one() -> f32 { 1.0 }

/// Parse a layout JSON into absolute key positions.
pub fn parse_json(json: &str) -> Result<Vec<KeycapPos>, String> {
    let layout: LayoutJson = serde_json::from_str(json)
        .map_err(|e| format!("Invalid layout JSON: {}", e))?;

    let mut out = Vec::new();

    // Top-level keys: absolute positions
    for k in &layout.keys {
        out.push(KeycapPos {
            row: k.row, col: k.col,
            x: k.x * UNIT_PX, y: k.y * UNIT_PX,
            w: k.w * UNIT_PX, h: k.h * UNIT_PX,
            angle: k.r,
        });
    }

    // Groups: apply rotation + translation to local coords
    for g in &layout.groups {
        let rad = g.r.to_radians();
        let cos_a = rad.cos();
        let sin_a = rad.sin();
        for k in &g.keys {
            let ax = g.x + k.x * cos_a - k.y * sin_a;
            let ay = g.y + k.x * sin_a + k.y * cos_a;
            out.push(KeycapPos {
                row: k.row, col: k.col,
                x: ax * UNIT_PX, y: ay * UNIT_PX,
                w: k.w * UNIT_PX, h: k.h * UNIT_PX,
                angle: g.r + k.r,
            });
        }
    }

    if out.is_empty() {
        return Err("No keys found in layout".into());
    }
    Ok(out)
}

/// Serialize absolute key positions back into a flat layout JSON.
///
/// Positions are converted from pixels back to keyboard units (px / UNIT_PX).
/// Any `groups` structure from the source is flattened away: every key becomes
/// a top-level entry with absolute coordinates and its combined angle.
pub fn serialize_json(keys: &[KeycapPos]) -> String {
    let entries: Vec<serde_json::Value> = keys
        .iter()
        .map(|k| {
            serde_json::json!({
                "row": k.row,
                "col": k.col,
                "x": round2(k.x / UNIT_PX),
                "y": round2(k.y / UNIT_PX),
                "w": round2(k.w / UNIT_PX),
                "h": round2(k.h / UNIT_PX),
                "r": round2(k.angle),
            })
        })
        .collect();
    let doc = serde_json::json!({ "keys": entries });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string())
}

/// Round to 2 decimals to keep the exported JSON tidy.
fn round2(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

/// Default layout embedded at compile time.
pub fn default_layout() -> Vec<KeycapPos> {
    let json = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/default.json"));
    parse_json(json).unwrap_or_default()
}
