use serde_json::Value;

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

const KEY_SIZE: f32 = 50.0;
const KEY_GAP: f32 = 4.0;

/// Parse a layout JSON string into absolute key positions.
pub fn parse_json(json: &str) -> Result<Vec<KeycapPos>, String> {
    let val: Value = serde_json::from_str(json)
        .map_err(|e| format!("Invalid layout JSON: {}", e))?;
    let mut keys = Vec::new();
    walk(&val, 0.0, 0.0, 0.0, &mut keys);
    if keys.is_empty() {
        return Err("No keys found in layout".into());
    }
    Ok(keys)
}

/// Default layout embedded at compile time.
pub fn default_layout() -> Vec<KeycapPos> {
    let json = include_str!("../default.json");
    parse_json(json).unwrap_or_default()
}

fn walk(node: &Value, ox: f32, oy: f32, parent_angle: f32, out: &mut Vec<KeycapPos>) {
    let obj = match node.as_object() {
        Some(o) => o,
        None => return,
    };

    for (key, val) in obj {
        let key_str = key.as_str();
        match key_str {
            "Group" => walk_group(val, ox, oy, parent_angle, out),
            "Line" => walk_line(val, ox, oy, parent_angle, out),
            "Keycap" => walk_keycap(val, ox, oy, parent_angle, out),
            _ => {}
        }
    }
}

fn parse_margin(val: &Value) -> (f32, f32, f32, f32) {
    let as_str = val.as_str();
    if let Some(s) = as_str {
        let split = s.split(',');
        let parts: Vec<f32> = split
            .filter_map(|p| {
                let trimmed = p.trim();
                let parsed = trimmed.parse().ok();
                parsed
            })
            .collect();
        let has_four_parts = parts.len() == 4;
        if has_four_parts {
            return (parts[0], parts[1], parts[2], parts[3]);
        }
    }
    (0.0, 0.0, 0.0, 0.0)
}

fn parse_angle(val: &Value) -> f32 {
    let rotate_transform = val.get("RotateTransform");
    let angle_val = rotate_transform
        .and_then(|rt| rt.get("Angle"));
    let angle_f64 = angle_val
        .and_then(|a| a.as_f64());
    let angle = angle_f64.unwrap_or(0.0) as f32;
    angle
}

fn walk_group(val: &Value, ox: f32, oy: f32, parent_angle: f32, out: &mut Vec<KeycapPos>) {
    let obj = match val.as_object() {
        Some(o) => o,
        None => return,
    };

    let margin_val = obj.get("Margin");
    let (ml, mt, _, _) = margin_val
        .map(parse_margin)
        .unwrap_or_default();
    let transform_val = obj.get("RenderTransform");
    let angle = transform_val
        .map(parse_angle)
        .unwrap_or(0.0);

    let gx = ox + ml;
    let gy = oy + mt;

    let children_val = obj.get("Children");
    let children_array = children_val
        .and_then(|c| c.as_array());
    if let Some(children) = children_array {
        let combined_angle = parent_angle + angle;
        for child in children {
            walk(child, gx, gy, combined_angle, out);
        }
    }
}

fn walk_line(val: &Value, ox: f32, oy: f32, parent_angle: f32, out: &mut Vec<KeycapPos>) {
    let obj = match val.as_object() {
        Some(o) => o,
        None => return,
    };

    let margin_val = obj.get("Margin");
    let (ml, mt, _, _) = margin_val
        .map(parse_margin)
        .unwrap_or_default();
    let transform_val = obj.get("RenderTransform");
    let angle = transform_val
        .map(parse_angle)
        .unwrap_or(0.0);
    let total_angle = parent_angle + angle;

    let orientation_val = obj.get("Orientation");
    let orientation_str = orientation_val
        .and_then(|o| o.as_str())
        .unwrap_or("Vertical");
    let horiz = orientation_str == "Horizontal";

    let lx = ox + ml;
    let ly = oy + mt;

    let rad = total_angle.to_radians();
    let cos_a = rad.cos();
    let sin_a = rad.sin();

    let mut cursor = 0.0f32;

    let children_val = obj.get("Children");
    let children_array = children_val
        .and_then(|c| c.as_array());
    if let Some(children) = children_array {
        for child in children {
            let (cx, cy) = if horiz {
                let x = lx + cursor * cos_a;
                let y = ly + cursor * sin_a;
                (x, y)
            } else {
                let x = lx - cursor * sin_a;
                let y = ly + cursor * cos_a;
                (x, y)
            };

            let child_size = measure(child, horiz);
            walk(child, cx, cy, total_angle, out);
            cursor += child_size;
        }
    }
}

/// Measure a child's extent along the parent's main axis.
fn measure(node: &Value, horiz: bool) -> f32 {
    let obj = match node.as_object() {
        Some(o) => o,
        None => return 0.0,
    };

    for (key, val) in obj {
        let key_str = key.as_str();
        match key_str {
            "Keycap" => {
                let width_val = val.get("Width");
                let width_f64 = width_val
                    .and_then(|v| v.as_f64());
                let w = width_f64.unwrap_or(KEY_SIZE as f64) as f32;
                let extent = if horiz {
                    w + KEY_GAP
                } else {
                    KEY_SIZE + KEY_GAP
                };
                return extent;
            }
            "Line" => {
                let sub = match val.as_object() {
                    Some(o) => o,
                    None => return 0.0,
                };
                let sub_orientation = sub.get("Orientation");
                let sub_orient_str = sub_orientation
                    .and_then(|o| o.as_str())
                    .unwrap_or("Vertical");
                let sub_horiz = sub_orient_str == "Horizontal";

                let sub_children_val = sub.get("Children");
                let sub_children_array = sub_children_val
                    .and_then(|c| c.as_array());
                let children = sub_children_array
                    .map(|a| a.as_slice())
                    .unwrap_or(&[]);

                let same_direction = sub_horiz == horiz;
                let content: f32 = if same_direction {
                    // Same direction: sum
                    children
                        .iter()
                        .map(|c| measure(c, sub_horiz))
                        .sum()
                } else {
                    // Cross direction: max
                    children
                        .iter()
                        .map(|c| measure(c, horiz))
                        .fold(0.0f32, f32::max)
                };

                return content;
            }
            "Group" => {
                let sub = match val.as_object() {
                    Some(o) => o,
                    None => return 0.0,
                };
                let sub_children_val = sub.get("Children");
                let sub_children_array = sub_children_val
                    .and_then(|c| c.as_array());
                let children = sub_children_array
                    .map(|a| a.as_slice())
                    .unwrap_or(&[]);
                let max_extent = children
                    .iter()
                    .map(|c| measure(c, horiz))
                    .fold(0.0f32, f32::max);
                return max_extent;
            }
            _ => {}
        }
    }
    0.0
}

fn walk_keycap(val: &Value, ox: f32, oy: f32, parent_angle: f32, out: &mut Vec<KeycapPos>) {
    let obj = match val.as_object() {
        Some(o) => o,
        None => return,
    };

    let col_val = obj.get("Column");
    let col_u64 = col_val
        .and_then(|v| v.as_u64());
    let col = col_u64.unwrap_or(0) as usize;

    let row_val = obj.get("Row");
    let row_u64 = row_val
        .and_then(|v| v.as_u64());
    let row = row_u64.unwrap_or(0) as usize;

    let width_val = obj.get("Width");
    let width_f64 = width_val
        .and_then(|v| v.as_f64());
    let w = width_f64.unwrap_or(KEY_SIZE as f64) as f32;

    let margin_val = obj.get("Margin");
    let (ml, mt, _, _) = margin_val
        .map(parse_margin)
        .unwrap_or_default();

    let transform_val = obj.get("RenderTransform");
    let angle = transform_val
        .map(parse_angle)
        .unwrap_or(0.0);

    let total_angle = parent_angle + angle;

    out.push(KeycapPos {
        row,
        col,
        x: ox + ml,
        y: oy + mt,
        w,
        h: KEY_SIZE,
        angle: total_angle,
    });
}
