use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::project::{SourceFormat, SourceRecord, SourceStatus};

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
        sources.push(SourceRecord {
            id: stable_source_id(root, &path),
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
            warnings: vec![],
        });
    }

    Ok(sources)
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
    if trimmed.is_empty() {
        "source".to_string()
    } else {
        trimmed.to_string()
    }
}
