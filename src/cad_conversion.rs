use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::project::SourceFormat;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CadConversionStatus {
    Success,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadConversionReport {
    pub generated_at: String,
    pub manifest_path: PathBuf,
    pub output_path: PathBuf,
    pub target_version: String,
    pub target_format: String,
    pub entries: Vec<CadConversionReportEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadConversionReportEntry {
    pub source_id: String,
    pub source_display_name: String,
    pub source_original_file_name: String,
    pub source_relative_path: PathBuf,
    pub input_path: PathBuf,
    pub input_format: SourceFormat,
    pub converted_path: Option<PathBuf>,
    pub converted_format: Option<String>,
    pub oda_version: Option<String>,
    pub target_version: String,
    pub target_format: String,
    pub success: bool,
    pub status: CadConversionStatus,
    pub input_sha256: String,
    pub converted_sha256: Option<String>,
    pub bbox_before: Option<Value>,
    pub bbox_after: Option<Value>,
    pub level_count_after: Option<usize>,
    pub material_count_after: Option<usize>,
    pub fingerprint_after: Option<Value>,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}
