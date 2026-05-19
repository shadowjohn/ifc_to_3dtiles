use anyhow::{Result, bail, ensure};
use serde_json::json;

use crate::{
    crs::project_to_wgs84,
    geometry::{Mesh, Vec3},
    glb::build_glb_with_extras,
    runtime_metadata::{
        RuntimeFeatureMetadata, RuntimeMetadataPayload, RuntimePickFeature, RuntimePickIndex,
    },
};

const MIN_BBOX_THICKNESS_M: f64 = 0.25;

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeFeatureGeometry {
    pub feature_id: i64,
    pub source_id: String,
    pub layer: String,
    pub geometry_type: String,
    pub material_id: String,
    pub bbox: [f64; 6],
}

#[derive(Debug, Clone)]
pub struct RuntimeProxyBuildOutput {
    pub glb: Vec<u8>,
    pub mesh: Mesh,
    pub metadata: RuntimeMetadataPayload,
    pub pick_index: RuntimePickIndex,
    pub triangle_count: usize,
    pub bbox_volume: f64,
}

pub fn build_runtime_proxy_glb(
    source_id: &str,
    origin_epsg3826: [f64; 3],
    origin_wgs84: [f64; 3],
    features: &[RuntimeFeatureGeometry],
) -> Result<RuntimeProxyBuildOutput> {
    ensure!(!features.is_empty(), "runtime source has no feature bbox");
    ensure!(
        features.len() <= u16::MAX as usize,
        "runtime source {source_id} has {} features; Phase 1H needs split runtime before u16 _BATCHID",
        features.len()
    );

    let mut mesh = Mesh::new();
    let mut metadata_features = Vec::with_capacity(features.len());
    let mut pick_features = Vec::with_capacity(features.len());
    let mut bbox_volume = 0.0;

    for (batch_id, feature) in features.iter().enumerate() {
        if feature.source_id != source_id {
            bail!(
                "runtime feature source mismatch: expected {source_id}, got {}",
                feature.source_id
            );
        }
        let expanded = expand_bbox(feature.bbox)?;
        let dims = bbox_dimensions(expanded);
        bbox_volume += dims[0] * dims[1] * dims[2];
        append_bbox_box(&mut mesh, expanded, origin_epsg3826, batch_id as u16);
        let explode_group_key = format!(
            "layer:{}",
            if feature.layer.trim().is_empty() {
                "(no layer)"
            } else {
                feature.layer.trim()
            }
        );
        let ifc_type = if feature.geometry_type.trim().is_empty() {
            "UNKNOWN".to_string()
        } else {
            feature.geometry_type.trim().to_string()
        };
        let material_id = if feature.material_id.trim().is_empty() {
            "default".to_string()
        } else {
            feature.material_id.trim().to_string()
        };
        metadata_features.push(RuntimeFeatureMetadata {
            feature_id: feature.feature_id,
            source_id: source_id.to_string(),
            explode_group_key: explode_group_key.clone(),
            ifc_type: ifc_type.clone(),
            material_id: material_id.clone(),
        });
        pick_features.push(RuntimePickFeature {
            batch_id: batch_id as u16,
            feature_id: feature.feature_id,
            source_id: source_id.to_string(),
            explode_group_key,
            ifc_type,
            material_id,
            bbox: expanded,
            bbox_wgs84: bbox_to_wgs84(expanded).ok(),
            center_wgs84: center_to_wgs84(expanded).ok(),
            dimensions: dims,
        });
    }

    let metadata = RuntimeMetadataPayload {
        source_id: source_id.to_string(),
        features: metadata_features,
    };
    let pick_index = RuntimePickIndex {
        source_id: source_id.to_string(),
        origin_epsg3826,
        origin_wgs84,
        features: pick_features,
    };
    let extras = json!({
        "runtime_source_id": source_id,
        "feature_count": features.len(),
        "batch_id_attribute": "_BATCHID",
        "metadata": "runtime_metadata.json",
        "pick_index": "runtime_pick.json"
    });
    let glb = build_glb_with_extras(&mesh, Some(extras))?;
    let triangle_count = mesh.triangle_count();

    Ok(RuntimeProxyBuildOutput {
        glb,
        mesh,
        metadata,
        pick_index,
        triangle_count,
        bbox_volume,
    })
}

fn expand_bbox(bbox: [f64; 6]) -> Result<[f64; 6]> {
    ensure!(
        bbox.iter().all(|value| value.is_finite()),
        "runtime bbox contains non-finite coordinate"
    );
    let mut out = bbox;
    for axis in 0..3 {
        if out[axis + 3] < out[axis] {
            out.swap(axis, axis + 3);
        }
        let size = out[axis + 3] - out[axis];
        if size < MIN_BBOX_THICKNESS_M {
            let center = (out[axis] + out[axis + 3]) * 0.5;
            out[axis] = center - MIN_BBOX_THICKNESS_M * 0.5;
            out[axis + 3] = center + MIN_BBOX_THICKNESS_M * 0.5;
        }
    }
    Ok(out)
}

fn bbox_dimensions(bbox: [f64; 6]) -> [f64; 3] {
    [
        (bbox[3] - bbox[0]).abs(),
        (bbox[4] - bbox[1]).abs(),
        (bbox[5] - bbox[2]).abs(),
    ]
}

fn bbox_to_wgs84(bbox: [f64; 6]) -> Result<[f64; 6]> {
    let min = project_to_wgs84(3826, bbox[0], bbox[1])?;
    let max = project_to_wgs84(3826, bbox[3], bbox[4])?;
    Ok([
        min.lon_deg.min(max.lon_deg),
        min.lat_deg.min(max.lat_deg),
        bbox[2],
        min.lon_deg.max(max.lon_deg),
        min.lat_deg.max(max.lat_deg),
        bbox[5],
    ])
}

fn center_to_wgs84(bbox: [f64; 6]) -> Result<[f64; 3]> {
    let lonlat = project_to_wgs84(3826, (bbox[0] + bbox[3]) * 0.5, (bbox[1] + bbox[4]) * 0.5)?;
    Ok([lonlat.lon_deg, lonlat.lat_deg, (bbox[2] + bbox[5]) * 0.5])
}

fn append_bbox_box(mesh: &mut Mesh, bbox: [f64; 6], origin: [f64; 3], batch_id: u16) {
    let corners = [
        local_point([bbox[0], bbox[1], bbox[2]], origin),
        local_point([bbox[3], bbox[1], bbox[2]], origin),
        local_point([bbox[3], bbox[4], bbox[2]], origin),
        local_point([bbox[0], bbox[4], bbox[2]], origin),
        local_point([bbox[0], bbox[1], bbox[5]], origin),
        local_point([bbox[3], bbox[1], bbox[5]], origin),
        local_point([bbox[3], bbox[4], bbox[5]], origin),
        local_point([bbox[0], bbox[4], bbox[5]], origin),
    ];
    let faces = [
        ([0, 1, 2, 3], [0.0, 0.0, -1.0]),
        ([4, 7, 6, 5], [0.0, 0.0, 1.0]),
        ([0, 4, 5, 1], [0.0, -1.0, 0.0]),
        ([1, 5, 6, 2], [1.0, 0.0, 0.0]),
        ([2, 6, 7, 3], [0.0, 1.0, 0.0]),
        ([3, 7, 4, 0], [-1.0, 0.0, 0.0]),
    ];
    for (indices, normal) in faces {
        append_triangle(
            mesh,
            corners[indices[0]],
            corners[indices[1]],
            corners[indices[2]],
            normal,
            batch_id,
        );
        append_triangle(
            mesh,
            corners[indices[0]],
            corners[indices[2]],
            corners[indices[3]],
            normal,
            batch_id,
        );
    }
}

fn local_point(point: [f64; 3], origin: [f64; 3]) -> [f64; 3] {
    [
        point[0] - origin[0],
        point[1] - origin[1],
        point[2] - origin[2],
    ]
}

fn append_triangle(
    mesh: &mut Mesh,
    a: [f64; 3],
    b: [f64; 3],
    c: [f64; 3],
    normal: [f64; 3],
    batch_id: u16,
) {
    for point in [a, b, c] {
        mesh.positions.push(point);
        mesh.normals.push(normal);
        mesh.colors.push([0.62, 0.78, 0.92, 0.35]);
        mesh.batch_ids.push(batch_id);
        mesh.bounds.include(Vec3::new(point[0], point[1], point[2]));
    }
}
