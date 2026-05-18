use anyhow::{Result, bail};
use serde_json::json;

use crate::geometry::Mesh;

pub fn build_glb(mesh: &Mesh) -> Result<Vec<u8>> {
    if mesh.positions.is_empty() {
        bail!("cannot build GLB for empty mesh");
    }
    if mesh.positions.len() > u32::MAX as usize {
        bail!("mesh has too many vertices");
    }

    let mut bin = Vec::new();
    let position_offset = bin.len();
    for p in &mesh.positions {
        for v in p {
            bin.extend_from_slice(&(*v as f32).to_le_bytes());
        }
    }
    pad4(&mut bin);

    let normal_offset = bin.len();
    for n in &mesh.normals {
        for v in n {
            bin.extend_from_slice(&(*v as f32).to_le_bytes());
        }
    }
    pad4(&mut bin);

    let color_offset = bin.len();
    for c in &mesh.colors {
        for v in c {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    pad4(&mut bin);

    let batch_offset = bin.len();
    for id in &mesh.batch_ids {
        bin.extend_from_slice(&id.to_le_bytes());
    }
    pad4(&mut bin);

    let count = mesh.positions.len();
    let (min, max) = position_min_max(&mesh.positions);
    let json_value = json!({
        "asset": { "version": "2.0", "generator": "ifc_to_3dtiles" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{
            "mesh": 0,
            "matrix": [1,0,0,0, 0,0,-1,0, 0,1,0,0, 0,0,0,1]
        }],
        "meshes": [{
            "primitives": [{
                "attributes": {
                    "POSITION": 0,
                    "NORMAL": 1,
                    "COLOR_0": 2,
                    "_BATCHID": 3
                },
                "material": 0,
                "mode": 4
            }]
        }],
        "materials": [{
            "doubleSided": true,
            "pbrMetallicRoughness": {
                "baseColorFactor": [1, 1, 1, 1],
                "metallicFactor": 0,
                "roughnessFactor": 0.9
            }
        }],
        "buffers": [{ "byteLength": bin.len() }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": position_offset, "byteLength": count * 12, "target": 34962 },
            { "buffer": 0, "byteOffset": normal_offset, "byteLength": count * 12, "target": 34962 },
            { "buffer": 0, "byteOffset": color_offset, "byteLength": count * 16, "target": 34962 },
            { "buffer": 0, "byteOffset": batch_offset, "byteLength": count * 2, "target": 34962 }
        ],
        "accessors": [
            { "bufferView": 0, "byteOffset": 0, "componentType": 5126, "count": count, "type": "VEC3", "min": min, "max": max },
            { "bufferView": 1, "byteOffset": 0, "componentType": 5126, "count": count, "type": "VEC3" },
            { "bufferView": 2, "byteOffset": 0, "componentType": 5126, "count": count, "type": "VEC4" },
            { "bufferView": 3, "byteOffset": 0, "componentType": 5123, "count": count, "type": "SCALAR" }
        ]
    });

    let mut json_bytes = serde_json::to_vec(&json_value)?;
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }

    let total_len = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total_len);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total_len as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    Ok(out)
}

fn pad4(bytes: &mut Vec<u8>) {
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
}

fn position_min_max(positions: &[[f64; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in positions {
        for i in 0..3 {
            min[i] = min[i].min(p[i] as f32);
            max[i] = max[i].max(p[i] as f32);
        }
    }
    (min, max)
}
