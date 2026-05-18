use serde_json::{Value, json};

use crate::geometry::Bounds;

#[derive(Debug, Clone)]
pub struct TileJson {
    pub uri: String,
    pub bounds: Bounds,
    pub geometric_error: f64,
}

pub fn box_array(bounds: &Bounds) -> [f64; 12] {
    let center = bounds.center();
    let size = bounds.size();
    [
        center.x,
        center.y,
        center.z,
        (size.x.abs() * 0.5).max(0.01),
        0.0,
        0.0,
        0.0,
        (size.y.abs() * 0.5).max(0.01),
        0.0,
        0.0,
        0.0,
        (size.z.abs() * 0.5).max(0.01),
    ]
}

pub fn build_tileset_json(
    transform: [f64; 16],
    root_bounds: &Bounds,
    children: &[TileJson],
) -> Value {
    let root_error = root_bounds.size().length().max(1.0);
    let child_values: Vec<Value> = children
        .iter()
        .map(|child| {
            json!({
                "boundingVolume": { "box": box_array(&child.bounds) },
                "geometricError": child.geometric_error,
                "content": {
                    "uri": child.uri,
                    "boundingVolume": { "box": box_array(&child.bounds) }
                }
            })
        })
        .collect();

    json!({
        "asset": {
            "version": "1.0",
            "generator": "ifc_to_3dtiles"
        },
        "geometricError": root_error,
        "root": {
            "transform": transform,
            "boundingVolume": { "box": box_array(root_bounds) },
            "geometricError": root_error,
            "refine": "ADD",
            "children": child_values
        }
    })
}
