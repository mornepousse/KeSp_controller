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
    name: Option<String>,
    rows: Option<usize>,
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

/// Géométrie déclarée par le layout de l'appareil, quand il en déclare une.
/// Une dimension nulle est traitée comme absente.
pub fn parse_geometry(json: &str) -> Option<(usize, usize)> {
    let layout: LayoutJson = serde_json::from_str(json).ok()?;
    match (layout.rows, layout.cols) {
        (Some(r), Some(c)) if r > 0 && c > 0 => Some((r, c)),
        _ => None,
    }
}

/// Nom déclaré par le layout de l'appareil. C'est la seule source qui dit de
/// quelle main est une demi-matrice — la géométrie ne le dit pas.
pub fn parse_name(json: &str) -> Option<String> {
    let layout: LayoutJson = serde_json::from_str(json).ok()?;
    layout.name.filter(|n| !n.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NIPHAR: &str = r#"{"name":"Niphargus Left","rows":4,"cols":7,"keys":[]}"#;

    #[test]
    fn geometry_is_read_from_the_layout_json() {
        assert_eq!(parse_geometry(NIPHAR), Some((4, 7)));
    }

    #[test]
    fn geometry_is_none_when_the_layout_omits_it() {
        assert_eq!(parse_geometry(r#"{"keys":[]}"#), None);
    }

    #[test]
    fn geometry_is_none_on_invalid_json() {
        assert_eq!(parse_geometry("not json"), None);
    }

    #[test]
    fn name_is_read_from_the_layout_json() {
        assert_eq!(parse_name(NIPHAR), Some("Niphargus Left".to_string()));
    }

    #[test]
    fn empty_name_counts_as_absent() {
        assert_eq!(parse_name(r#"{"name":"","keys":[]}"#), None);
    }
}
