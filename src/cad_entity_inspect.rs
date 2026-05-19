use std::{collections::BTreeMap, path::Path};

use anyhow::Result;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::georef::{Aoi, Bounds2, BoundsSummary, classify_source_scale};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WktCoordinateScan {
    pub geometry_type: String,
    pub has_z: bool,
    pub points: Vec<[f64; 3]>,
    pub bbox: [f64; 6],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadEntity {
    pub source_id: String,
    pub fid: i64,
    pub layer: String,
    pub subclasses: Option<String>,
    pub linetype: Option<String>,
    pub entity_handle: Option<String>,
    pub geometry_type: Option<String>,
    pub vertex_count: u64,
    pub has_z: bool,
    pub bbox: Option<[f64; 6]>,
    pub warnings: Vec<String>,
}

impl CadEntity {
    pub fn with_bbox(
        source_id: impl Into<String>,
        fid: i64,
        layer: impl Into<String>,
        geometry_type: impl Into<String>,
        bbox: [f64; 6],
        vertex_count: u64,
        has_z: bool,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            fid,
            layer: layer.into(),
            subclasses: None,
            linetype: None,
            entity_handle: None,
            geometry_type: Some(geometry_type.into()),
            vertex_count,
            has_z,
            bbox: Some(bbox),
            warnings: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadEntityStats {
    pub source_id: String,
    pub entity_count: u64,
    pub parsed_entity_count: u64,
    pub skipped_entity_count: u64,
    pub vertex_count: u64,
    pub raw_bbox: [f64; 6],
    pub percentile_bbox: [f64; 6],
    pub z_range: f64,
    pub selected_scale: Option<f64>,
    pub inspect_status: String,
    pub layer_histogram: BTreeMap<String, u64>,
    pub geometry_type_histogram: BTreeMap<String, u64>,
    pub fingerprint_hash: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityInspectReport {
    pub generated_at: String,
    pub conversion_report_path: std::path::PathBuf,
    pub project_db_path: std::path::PathBuf,
    pub entity_count: u64,
    pub parsed_entity_count: u64,
    pub skipped_entity_count: u64,
    pub sources: Vec<CadEntityStats>,
    pub warnings: Vec<String>,
}

pub fn scan_wkt_coordinates(wkt: &str) -> std::result::Result<WktCoordinateScan, String> {
    let trimmed = wkt.trim();
    let geometry_type = trimmed
        .split_whitespace()
        .next()
        .ok_or_else(|| "empty WKT".to_string())?
        .trim()
        .to_ascii_uppercase();
    let has_z = trimmed
        .get(geometry_type.len()..)
        .unwrap_or("")
        .trim_start()
        .starts_with('Z');
    let dimension = if has_z { 3 } else { 2 };
    let numbers = scan_numbers(trimmed);
    if numbers.len() < dimension {
        return Err("WKT has no coordinate tuples".to_string());
    }

    let mut points = Vec::new();
    for chunk in numbers.chunks(dimension) {
        if chunk.len() != dimension {
            break;
        }
        let z = if has_z { chunk[2] } else { 0.0 };
        points.push([chunk[0], chunk[1], z]);
    }
    if points.is_empty() {
        return Err("WKT has no complete coordinate tuples".to_string());
    }

    let bbox = bbox_from_points(&points);
    Ok(WktCoordinateScan {
        geometry_type,
        has_z,
        points,
        bbox,
    })
}

pub fn parse_ogrinfo_entities(source_id: &str, text: &str) -> Vec<CadEntity> {
    let mut entities = Vec::new();
    let mut current: Option<CadEntity> = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(fid) = parse_ogr_feature_fid(trimmed) {
            if let Some(entity) = current.take() {
                entities.push(entity);
            }
            current = Some(CadEntity {
                source_id: source_id.to_string(),
                fid,
                layer: String::new(),
                subclasses: None,
                linetype: None,
                entity_handle: None,
                geometry_type: None,
                vertex_count: 0,
                has_z: false,
                bbox: None,
                warnings: vec![],
            });
            continue;
        }

        let Some(entity) = current.as_mut() else {
            continue;
        };

        if let Some(value) = parse_ogr_field(trimmed, "Layer") {
            entity.layer = value;
        } else if let Some(value) = parse_ogr_field(trimmed, "SubClasses") {
            entity.subclasses = Some(value);
        } else if let Some(value) = parse_ogr_field(trimmed, "Linetype") {
            entity.linetype = Some(value);
        } else if let Some(value) = parse_ogr_field(trimmed, "EntityHandle") {
            entity.entity_handle = Some(value);
        } else if looks_like_wkt(trimmed) {
            match scan_wkt_coordinates(trimmed) {
                Ok(scan) => {
                    entity.geometry_type = Some(scan.geometry_type);
                    entity.vertex_count = scan.points.len() as u64;
                    entity.has_z = scan.has_z;
                    entity.bbox = Some(scan.bbox);
                }
                Err(err) => entity.warnings.push(err),
            }
        }
    }

    if let Some(entity) = current.take() {
        entities.push(entity);
    }

    for entity in &mut entities {
        if entity.layer.is_empty() {
            entity.layer = "[no_layer]".to_string();
        }
        if entity.bbox.is_none() {
            entity
                .warnings
                .push("entity geometry unavailable or unsupported".to_string());
        }
    }

    entities
}

pub fn summarize_entities(
    source_id: &str,
    entities: &[CadEntity],
    allowed_scales: &[f64],
) -> CadEntityStats {
    let mut layer_histogram = BTreeMap::new();
    let mut geometry_type_histogram = BTreeMap::new();
    let mut vertex_count = 0_u64;
    let mut parsed = Vec::new();

    for entity in entities {
        *layer_histogram.entry(entity.layer.clone()).or_insert(0) += 1;
        if let Some(geometry_type) = &entity.geometry_type {
            *geometry_type_histogram
                .entry(geometry_type.clone())
                .or_insert(0) += 1;
        }
        vertex_count += entity.vertex_count;
        if let Some(bbox) = entity.bbox {
            parsed.push(bbox);
        }
    }

    let raw_bbox = merge_bboxes(&parsed).unwrap_or([0.0; 6]);
    let percentile_bbox = percentile_bbox(&parsed, 0.5, 99.5).unwrap_or(raw_bbox);
    let z_range = (percentile_bbox[5] - percentile_bbox[2]).abs();
    let bounds = BoundsSummary::from_raw_and_percentile(
        Bounds2::new(raw_bbox[0], raw_bbox[1], raw_bbox[3], raw_bbox[4]),
        Bounds2::new(
            percentile_bbox[0],
            percentile_bbox[1],
            percentile_bbox[3],
            percentile_bbox[4],
        ),
    );
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let scale = classify_source_scale(&bounds, &aoi, allowed_scales);
    let mut warnings = scale.warnings;
    let inspect_status = if parsed.is_empty() {
        warnings.push("no parseable entity geometry".to_string());
        "quarantined"
    } else if scale.selected_scale.is_some() {
        "approved"
    } else {
        "quarantined"
    }
    .to_string();

    let mut stats = CadEntityStats {
        source_id: source_id.to_string(),
        entity_count: entities.len() as u64,
        parsed_entity_count: parsed.len() as u64,
        skipped_entity_count: entities.len().saturating_sub(parsed.len()) as u64,
        vertex_count,
        raw_bbox,
        percentile_bbox,
        z_range,
        selected_scale: scale.selected_scale,
        inspect_status,
        layer_histogram,
        geometry_type_histogram,
        fingerprint_hash: String::new(),
        warnings,
    };
    stats.fingerprint_hash = entity_fingerprint_hash(&stats);
    stats
}

pub fn entity_fingerprint_hash(stats: &CadEntityStats) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    fn add(hash: &mut u64, text: &str) {
        for byte in text.as_bytes() {
            *hash ^= u64::from(*byte);
            *hash = hash.wrapping_mul(0x100000001b3);
        }
    }

    add(&mut hash, &format!("entities={};", stats.entity_count));
    add(&mut hash, &format!("vertices={};", stats.vertex_count));
    add(&mut hash, &format!("scale={:?};", stats.selected_scale));
    for value in stats.percentile_bbox {
        add(&mut hash, &format!("{value:.3};"));
    }
    add(&mut hash, &format!("z={:.3};", stats.z_range));
    for (key, value) in &stats.layer_histogram {
        add(&mut hash, &format!("L:{key}={value};"));
    }
    for (key, value) in &stats.geometry_type_histogram {
        add(&mut hash, &format!("G:{key}={value};"));
    }
    format!("{hash:016x}")
}

pub fn write_entity_inspect_db(
    path: &Path,
    entities: &[CadEntity],
    stats: &[CadEntityStats],
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let mut conn = Connection::open(path)?;
    create_schema(&conn)?;
    let tx = conn.transaction()?;

    for source_stats in stats {
        tx.execute(
            "INSERT INTO sources(source_id, inspect_status, selected_scale, fingerprint_hash)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                source_stats.source_id,
                source_stats.inspect_status,
                source_stats.selected_scale,
                source_stats.fingerprint_hash
            ],
        )?;
        tx.execute(
            "INSERT INTO source_stats(
                source_id, entity_count, parsed_entity_count, skipped_entity_count,
                vertex_count, raw_bbox_json, percentile_bbox_json, z_range,
                selected_scale, layer_histogram_json, geometry_type_histogram_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                source_stats.source_id,
                source_stats.entity_count,
                source_stats.parsed_entity_count,
                source_stats.skipped_entity_count,
                source_stats.vertex_count,
                serde_json::to_string(&source_stats.raw_bbox)?,
                serde_json::to_string(&source_stats.percentile_bbox)?,
                source_stats.z_range,
                source_stats.selected_scale,
                serde_json::to_string(&source_stats.layer_histogram)?,
                serde_json::to_string(&source_stats.geometry_type_histogram)?,
            ],
        )?;
        tx.execute(
            "INSERT INTO fingerprints(source_id, fingerprint_hash, inputs_json)
             VALUES (?1, ?2, ?3)",
            params![
                source_stats.source_id,
                source_stats.fingerprint_hash,
                serde_json::to_string(source_stats)?,
            ],
        )?;
        for warning in &source_stats.warnings {
            tx.execute(
                "INSERT INTO warnings(source_id, scope, message) VALUES (?1, 'source', ?2)",
                params![source_stats.source_id, warning],
            )?;
        }
    }

    for entity in entities {
        tx.execute(
            "INSERT INTO entities(
                source_id, fid, layer, subclasses, linetype, entity_handle,
                geometry_type, vertex_count, has_z
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                entity.source_id,
                entity.fid,
                entity.layer,
                entity.subclasses,
                entity.linetype,
                entity.entity_handle,
                entity.geometry_type,
                entity.vertex_count,
                entity.has_z,
            ],
        )?;
        if let Some(bbox) = entity.bbox {
            tx.execute(
                "INSERT INTO entity_bboxes(source_id, fid, min_x, min_y, min_z, max_x, max_y, max_z)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    entity.source_id,
                    entity.fid,
                    bbox[0],
                    bbox[1],
                    bbox[2],
                    bbox[3],
                    bbox[4],
                    bbox[5]
                ],
            )?;
        }
        for warning in &entity.warnings {
            tx.execute(
                "INSERT INTO warnings(source_id, fid, scope, message) VALUES (?1, ?2, 'entity', ?3)",
                params![entity.source_id, entity.fid, warning],
            )?;
        }
    }

    tx.commit()?;
    Ok(())
}

fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS sources (
          source_id TEXT PRIMARY KEY,
          inspect_status TEXT NOT NULL,
          selected_scale REAL,
          fingerprint_hash TEXT
        );
        CREATE TABLE IF NOT EXISTS entities (
          source_id TEXT NOT NULL,
          fid INTEGER NOT NULL,
          layer TEXT NOT NULL,
          subclasses TEXT,
          linetype TEXT,
          entity_handle TEXT,
          geometry_type TEXT,
          vertex_count INTEGER NOT NULL,
          has_z INTEGER NOT NULL,
          PRIMARY KEY (source_id, fid)
        );
        CREATE TABLE IF NOT EXISTS entity_bboxes (
          source_id TEXT NOT NULL,
          fid INTEGER NOT NULL,
          min_x REAL NOT NULL,
          min_y REAL NOT NULL,
          min_z REAL NOT NULL,
          max_x REAL NOT NULL,
          max_y REAL NOT NULL,
          max_z REAL NOT NULL,
          PRIMARY KEY (source_id, fid)
        );
        CREATE TABLE IF NOT EXISTS source_stats (
          source_id TEXT PRIMARY KEY,
          entity_count INTEGER NOT NULL,
          parsed_entity_count INTEGER NOT NULL,
          skipped_entity_count INTEGER NOT NULL,
          vertex_count INTEGER NOT NULL,
          raw_bbox_json TEXT NOT NULL,
          percentile_bbox_json TEXT NOT NULL,
          z_range REAL NOT NULL,
          selected_scale REAL,
          layer_histogram_json TEXT NOT NULL,
          geometry_type_histogram_json TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS warnings (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          source_id TEXT NOT NULL,
          fid INTEGER,
          scope TEXT NOT NULL,
          message TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS fingerprints (
          source_id TEXT PRIMARY KEY,
          fingerprint_hash TEXT NOT NULL,
          inputs_json TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS conversion_runs (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          source_id TEXT,
          target_version TEXT,
          target_format TEXT,
          success INTEGER,
          report_json TEXT
        );
        ",
    )?;
    Ok(())
}

fn looks_like_wkt(text: &str) -> bool {
    const TYPES: [&str; 12] = [
        "POINT",
        "LINESTRING",
        "POLYGON",
        "MULTIPOINT",
        "MULTILINESTRING",
        "MULTIPOLYGON",
        "GEOMETRYCOLLECTION",
        "POLYHEDRALSURFACE",
        "TIN",
        "TRIANGLE",
        "CIRCULARSTRING",
        "COMPOUNDCURVE",
    ];
    let upper = text.to_ascii_uppercase();
    TYPES.iter().any(|prefix| upper.starts_with(prefix))
}

fn parse_ogr_feature_fid(text: &str) -> Option<i64> {
    let marker = "OGRFeature(";
    if !text.starts_with(marker) {
        return None;
    }
    let index = text.find("):")?;
    text[index + 2..].trim().parse().ok()
}

fn parse_ogr_field(text: &str, field_name: &str) -> Option<String> {
    if !text.starts_with(field_name) {
        return None;
    }
    let index = text.find(" = ")?;
    Some(text[index + 3..].trim().to_string())
}

fn scan_numbers(text: &str) -> Vec<f64> {
    let mut values = Vec::new();
    let mut token = String::new();
    let mut in_number = false;

    for ch in text.chars() {
        let numeric =
            ch.is_ascii_digit() || ch == '-' || ch == '+' || ch == '.' || ch == 'e' || ch == 'E';
        if numeric {
            token.push(ch);
            in_number = true;
        } else if in_number {
            if let Ok(value) = token.parse::<f64>() {
                values.push(value);
            }
            token.clear();
            in_number = false;
        }
    }
    if in_number {
        if let Ok(value) = token.parse::<f64>() {
            values.push(value);
        }
    }

    values
}

fn bbox_from_points(points: &[[f64; 3]]) -> [f64; 6] {
    let mut bbox = [
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for point in points {
        bbox[0] = bbox[0].min(point[0]);
        bbox[1] = bbox[1].min(point[1]);
        bbox[2] = bbox[2].min(point[2]);
        bbox[3] = bbox[3].max(point[0]);
        bbox[4] = bbox[4].max(point[1]);
        bbox[5] = bbox[5].max(point[2]);
    }
    bbox
}

fn merge_bboxes(bboxes: &[[f64; 6]]) -> Option<[f64; 6]> {
    let mut iter = bboxes.iter();
    let first = *iter.next()?;
    let mut merged = first;
    for bbox in iter {
        merged[0] = merged[0].min(bbox[0]);
        merged[1] = merged[1].min(bbox[1]);
        merged[2] = merged[2].min(bbox[2]);
        merged[3] = merged[3].max(bbox[3]);
        merged[4] = merged[4].max(bbox[4]);
        merged[5] = merged[5].max(bbox[5]);
    }
    Some(merged)
}

fn percentile_bbox(bboxes: &[[f64; 6]], low: f64, high: f64) -> Option<[f64; 6]> {
    if bboxes.is_empty() {
        return None;
    }
    let mut xs = Vec::with_capacity(bboxes.len() * 2);
    let mut ys = Vec::with_capacity(bboxes.len() * 2);
    let mut zs = Vec::with_capacity(bboxes.len() * 2);
    for bbox in bboxes {
        xs.push(bbox[0]);
        xs.push(bbox[3]);
        ys.push(bbox[1]);
        ys.push(bbox[4]);
        zs.push(bbox[2]);
        zs.push(bbox[5]);
    }
    xs.sort_by(f64::total_cmp);
    ys.sort_by(f64::total_cmp);
    zs.sort_by(f64::total_cmp);
    Some([
        percentile_value(&xs, low),
        percentile_value(&ys, low),
        percentile_value(&zs, low),
        percentile_value(&xs, high),
        percentile_value(&ys, high),
        percentile_value(&zs, high),
    ])
}

fn percentile_value(sorted: &[f64], percentile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (percentile / 100.0) * ((sorted.len() - 1) as f64);
    let index = rank.round().clamp(0.0, (sorted.len() - 1) as f64) as usize;
    sorted[index]
}
