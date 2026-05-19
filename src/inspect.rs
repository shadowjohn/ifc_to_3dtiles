use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::cad_metadata::CadHierarchyDump;
use crate::project::{SourceFormat, SourceRecord, SourceStatus};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadProbeSummary {
    pub tools: CadProbeTools,
    #[serde(default)]
    pub oda_file_converters: Vec<CadOdaToolSummary>,
    pub preferred_oda_file_converter: CadOdaToolSummary,
    pub file_count: usize,
    pub cad_file_count: usize,
    #[serde(default)]
    pub extension_distribution: Vec<CadExtensionSummary>,
    #[serde(default)]
    pub cad_files: Vec<CadProbeFile>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadProbeTools {
    pub ogrinfo: CadToolSummary,
    pub ogr2ogr: CadToolSummary,
    pub oda_file_converter: CadToolSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadToolSummary {
    pub found: bool,
    pub source: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub version_risk: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadOdaToolSummary {
    pub found: bool,
    pub name: String,
    pub source: Option<String>,
    pub version: Option<String>,
    pub version_major: u32,
    pub version_minor: u32,
    pub version_build: i32,
    pub version_revision: i32,
    pub version_risk: Option<String>,
    pub last_write_time: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadExtensionSummary {
    pub extension: String,
    pub count: usize,
    pub total_bytes: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadProbeFile {
    #[serde(rename = "FullName")]
    pub full_name: PathBuf,
    #[serde(rename = "Extension")]
    pub extension: String,
    #[serde(rename = "Length")]
    pub length: u64,
}

pub fn read_cad_probe_summary(path: &Path) -> Result<CadProbeSummary> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("讀取 CAD probe 失敗：{}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("解析 CAD probe JSON 失敗：{}", path.display()))
}

pub fn source_format_from_path(path: impl AsRef<Path>) -> SourceFormat {
    match path
        .as_ref()
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
    {
        Some(ext) if ext == "ifc" => SourceFormat::Ifc,
        Some(ext) if ext == "rvt" => SourceFormat::Rvt,
        Some(ext) if ext == "dgn" => SourceFormat::Dgn,
        Some(ext) if ext == "dwg" => SourceFormat::Dwg,
        _ => SourceFormat::Unknown,
    }
}

pub fn discover_sources(root: &Path) -> Result<Vec<SourceRecord>> {
    let mut files = Vec::<PathBuf>::new();
    collect_files(root, &mut files)?;
    files.sort();

    let mut sources = Vec::new();
    for path in files {
        let format = source_format_from_path(&path);
        if format == SourceFormat::Unknown {
            continue;
        }

        let metadata =
            fs::metadata(&path).with_context(|| format!("讀取檔案資訊失敗：{}", path.display()))?;
        let relative_path = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        let original_file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("source")
            .to_string();
        let display_name = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or(&original_file_name)
            .to_string();
        let mut warnings = Vec::new();
        if has_repeated_cad_like_extension(&original_file_name) {
            warnings.push(
                "possible_intermediate_or_export_copy: file has repeated CAD-like extensions"
                    .to_string(),
            );
        }
        sources.push(SourceRecord {
            id: stable_source_id(root, &path),
            display_name,
            original_file_name,
            relative_path,
            path,
            format,
            status: SourceStatus::PendingInspect,
            original_size_bytes: metadata.len(),
            detected_crs: None,
            unit_scale_to_meter: None,
            anchor_distance_m: None,
            raw_bbox: None,
            percentile_bbox: None,
            transform: None,
            cad_metadata_path: None,
            fingerprint_hash: None,
            duplicate_candidates: vec![],
            warnings,
        });
    }

    Ok(sources)
}

pub fn write_empty_cad_metadata_dumps(sources: &mut [SourceRecord], output: &Path) -> Result<()> {
    let metadata_dir = output.join("cad_metadata");
    fs::create_dir_all(&metadata_dir)
        .with_context(|| format!("建立 CAD metadata 目錄失敗：{}", metadata_dir.display()))?;
    for entry in fs::read_dir(&metadata_dir)
        .with_context(|| format!("讀取 CAD metadata 目錄失敗：{}", metadata_dir.display()))?
    {
        let path = entry?.path();
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        {
            fs::remove_file(&path)
                .with_context(|| format!("刪除舊 CAD metadata 失敗：{}", path.display()))?;
        }
    }

    for source in sources {
        if !matches!(source.format, SourceFormat::Dgn | SourceFormat::Dwg) {
            continue;
        }

        let relative_path = PathBuf::from("cad_metadata").join(format!("{}.json", source.id));
        let output_path = output.join(&relative_path);
        let dump = CadHierarchyDump::empty_for_source(&source.id);
        fs::write(&output_path, serde_json::to_vec_pretty(&dump)?)
            .with_context(|| format!("寫入 CAD metadata 失敗：{}", output_path.display()))?;
        source.cad_metadata_path = Some(relative_path);
        source
            .warnings
            .push("CAD hierarchy probe unavailable; wrote empty metadata buckets".to_string());
    }

    Ok(())
}

fn collect_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root).with_context(|| format!("讀取目錄失敗：{}", root.display()))?
    {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

fn stable_source_id(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut id = String::new();
    let mut previous_dash = false;

    for ch in relative.to_string_lossy().chars() {
        if ch.is_ascii_alphanumeric() {
            id.push(ch.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash {
            id.push('-');
            previous_dash = true;
        }
    }

    let trimmed = id.trim_matches('-');
    let slug = if trimmed.is_empty() {
        "source".to_string()
    } else {
        trimmed.to_string()
    };
    format!("{slug}-{:08x}", stable_hash(relative))
}

fn has_repeated_cad_like_extension(file_name: &str) -> bool {
    let lowered = file_name.to_ascii_lowercase();
    let cad_tokens = [".dgn.", ".dwg.", ".dxf.", ".ifc."];
    cad_tokens.iter().any(|token| lowered.contains(token))
}

fn stable_hash(path: &Path) -> u32 {
    let mut hash = 0x811c9dc5_u32;
    for byte in path.to_string_lossy().to_lowercase().as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}
