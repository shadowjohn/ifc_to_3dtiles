use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub const SPATIAL_PICK_INDEX_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpatialPickIndex {
    pub version: u32,
    pub crs: String,
    pub sources: Vec<SpatialPickSource>,
    pub features: Vec<SpatialPickFeature>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpatialPickSource {
    pub source_id: String,
    pub origin_epsg3826: [f64; 3],
    pub origin_wgs84: [f64; 3],
    pub model_matrix: [f64; 16],
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpatialPickSourceInput {
    pub source_id: String,
    pub origin_epsg3826: [f64; 3],
    pub origin_wgs84: [f64; 3],
    pub model_matrix: [f64; 16],
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpatialPickFeatureInput {
    pub feature_id: i64,
    pub source_id: String,
    pub layer: String,
    pub name: Option<String>,
    pub category: String,
    pub bbox: Option<[f64; 6]>,
    pub metadata_ref: SpatialPickMetadataRef,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpatialPickMetadataRef {
    pub global_id: Option<String>,
    pub express_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpatialPickFeature {
    pub feature_id: i64,
    pub source_id: String,
    pub layer: String,
    pub name: String,
    pub category: String,
    pub bbox: [f64; 6],
    pub center: [f64; 3],
    pub radius: f64,
    pub metadata_ref: SpatialPickMetadataRef,
}

pub fn build_spatial_pick_index(
    crs: impl Into<String>,
    sources: &[SpatialPickSourceInput],
    features: &[SpatialPickFeatureInput],
) -> SpatialPickIndex {
    let source_map: HashMap<_, _> = sources
        .iter()
        .map(|source| (source.source_id.as_str(), source))
        .collect();
    let mut warnings = Vec::new();
    let mut pick_features = Vec::new();

    for feature in features {
        let Some(source) = source_map.get(feature.source_id.as_str()) else {
            warnings.push(format!(
                "feature {} source {} missing from spatial pick sources",
                feature.feature_id, feature.source_id
            ));
            continue;
        };
        let Some(bbox) = feature.bbox else {
            warnings.push(format!(
                "feature {} missing bbox; skipped spatial pick index",
                feature.feature_id
            ));
            continue;
        };
        if !bbox.iter().all(|value| value.is_finite()) {
            warnings.push(format!(
                "feature {} has non-finite bbox; skipped spatial pick index",
                feature.feature_id
            ));
            continue;
        }
        let normalized = normalize_bbox(bbox);
        let local = local_bbox(normalized, source.origin_epsg3826);
        let center = bbox_center(local);
        let radius = bbox_radius(local);
        let layer = if feature.layer.trim().is_empty() {
            "(no layer)".to_string()
        } else {
            feature.layer.trim().to_string()
        };
        let category = if feature.category.trim().is_empty() {
            "UNKNOWN".to_string()
        } else {
            feature.category.trim().to_string()
        };
        let name = feature
            .name
            .clone()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| format!("{layer} FID {}", feature.feature_id));
        pick_features.push(SpatialPickFeature {
            feature_id: feature.feature_id,
            source_id: feature.source_id.clone(),
            layer,
            name,
            category,
            bbox: local,
            center,
            radius,
            metadata_ref: feature.metadata_ref.clone(),
        });
    }

    SpatialPickIndex {
        version: SPATIAL_PICK_INDEX_VERSION,
        crs: crs.into(),
        sources: sources
            .iter()
            .map(|source| SpatialPickSource {
                source_id: source.source_id.clone(),
                origin_epsg3826: source.origin_epsg3826,
                origin_wgs84: source.origin_wgs84,
                model_matrix: source.model_matrix,
            })
            .collect(),
        features: pick_features,
        warnings,
    }
}

fn normalize_bbox(bbox: [f64; 6]) -> [f64; 6] {
    [
        bbox[0].min(bbox[3]),
        bbox[1].min(bbox[4]),
        bbox[2].min(bbox[5]),
        bbox[0].max(bbox[3]),
        bbox[1].max(bbox[4]),
        bbox[2].max(bbox[5]),
    ]
}

fn local_bbox(bbox: [f64; 6], origin: [f64; 3]) -> [f64; 6] {
    [
        bbox[0] - origin[0],
        bbox[1] - origin[1],
        bbox[2] - origin[2],
        bbox[3] - origin[0],
        bbox[4] - origin[1],
        bbox[5] - origin[2],
    ]
}

fn bbox_center(bbox: [f64; 6]) -> [f64; 3] {
    [
        (bbox[0] + bbox[3]) * 0.5,
        (bbox[1] + bbox[4]) * 0.5,
        (bbox[2] + bbox[5]) * 0.5,
    ]
}

fn bbox_radius(bbox: [f64; 6]) -> f64 {
    let dx = bbox[3] - bbox[0];
    let dy = bbox[4] - bbox[1];
    let dz = bbox[5] - bbox[2];
    (dx * dx + dy * dy + dz * dz).sqrt() * 0.5
}
