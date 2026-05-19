use std::collections::BTreeSet;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const RUNTIME_VERSION: u32 = 1;
pub const RUNTIME_METADATA_FIELDS: [&str; 5] = [
    "feature_id",
    "source_id",
    "explode_group_key",
    "ifc_type",
    "material_id",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeManifest {
    pub runtime_version: u32,
    pub approved_source_count: usize,
    pub sources: Vec<RuntimeManifestSource>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeManifestSource {
    pub source_id: String,
    pub display_name: String,
    pub geometry_path: String,
    pub status: String,
    pub selected_scale: Option<f64>,
    pub feature_count: usize,
    pub vertex_count: u64,
    pub bbox_percentile: Option<[f64; 6]>,
    pub runtime_metadata_fields: Vec<String>,
    pub model_matrix: [f64; 16],
    pub origin_epsg3826: [f64; 3],
    pub origin_wgs84: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeMetadataPayload {
    pub source_id: String,
    pub features: Vec<RuntimeFeatureMetadata>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeFeatureMetadata {
    pub feature_id: i64,
    pub source_id: String,
    pub explode_group_key: String,
    pub ifc_type: String,
    pub material_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimePickIndex {
    pub source_id: String,
    pub origin_epsg3826: [f64; 3],
    pub origin_wgs84: [f64; 3],
    pub features: Vec<RuntimePickFeature>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimePickFeature {
    pub batch_id: u16,
    pub feature_id: i64,
    pub source_id: String,
    pub explode_group_key: String,
    pub ifc_type: String,
    pub material_id: String,
    pub bbox: [f64; 6],
    pub bbox_wgs84: Option<[f64; 6]>,
    pub center_wgs84: Option<[f64; 3]>,
    pub dimensions: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeBudgetReport {
    pub runtime_version: u32,
    pub sources: Vec<RuntimeBudgetSource>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeBudgetSource {
    pub source_id: String,
    pub triangle_count: usize,
    pub vertex_count: usize,
    pub runtime_metadata_bytes: usize,
    pub bbox_volume: f64,
    pub geometry_file_size: u64,
}

pub fn runtime_metadata_field_names() -> Vec<String> {
    RUNTIME_METADATA_FIELDS
        .iter()
        .map(|field| field.to_string())
        .collect()
}

pub fn validate_runtime_metadata_fields(value: &Value) -> Result<()> {
    validate_forbidden_keys(value)?;
    let features = value
        .get("features")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("runtime metadata must contain features array"))?;
    let allowed: BTreeSet<_> = RUNTIME_METADATA_FIELDS.iter().copied().collect();
    for feature in features {
        let object = feature
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("runtime feature metadata must be an object"))?;
        for key in object.keys() {
            if !allowed.contains(key.as_str()) {
                bail!("runtime metadata field is not allowed: {key}");
            }
        }
        for required in RUNTIME_METADATA_FIELDS {
            if !object.contains_key(required) {
                bail!("runtime metadata missing field: {required}");
            }
        }
    }
    Ok(())
}

fn validate_forbidden_keys(value: &Value) -> Result<()> {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let lower = key.to_ascii_lowercase();
                if matches!(
                    lower.as_str(),
                    "psets_json"
                        | "cad_hierarchy"
                        | "hierarchy"
                        | "raw_properties"
                        | "properties"
                        | "full_property_dump"
                        | "cad_metadata"
                        | "models"
                        | "references"
                        | "levels"
                        | "materials"
                ) {
                    bail!("runtime metadata contains forbidden field: {key}");
                }
                if let Some(text) = child.as_str() {
                    if text.len() > 512 {
                        bail!("runtime metadata contains oversized text field: {key}");
                    }
                }
                validate_forbidden_keys(child)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                validate_forbidden_keys(item)?;
            }
        }
        _ => {}
    }
    Ok(())
}
