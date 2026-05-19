use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceLayout {
    pub root: PathBuf,
    pub sources: PathBuf,
    pub staging: PathBuf,
    pub normalized: PathBuf,
    pub publish: PathBuf,
}

impl WorkspaceLayout {
    pub fn new(root: PathBuf) -> Self {
        Self {
            sources: root.join("sources"),
            staging: root.join("staging"),
            normalized: root.join("normalized"),
            publish: root.join("publish"),
            root,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceFormat {
    Ifc,
    Rvt,
    Dgn,
    Dwg,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceStatus {
    PendingInspect,
    Approved,
    Quarantined,
    Converted,
    Published,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceRecord {
    pub id: String,
    pub path: PathBuf,
    pub format: SourceFormat,
    pub status: SourceStatus,
    pub original_size_bytes: u64,
    pub detected_crs: Option<String>,
    pub unit_scale_to_meter: Option<f64>,
    pub anchor_distance_m: Option<f64>,
    pub raw_bbox: Option<[f64; 6]>,
    pub percentile_bbox: Option<[f64; 6]>,
    pub transform: Option<serde_json::Value>,
    pub cad_metadata_path: Option<PathBuf>,
    pub fingerprint_hash: Option<String>,
    pub duplicate_candidates: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub project_id: String,
    pub source_epsg: u32,
    pub anchor_source_id: Option<String>,
    pub allowed_scales: Vec<f64>,
    pub sources: Vec<SourceRecord>,
}
