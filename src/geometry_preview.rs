use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::Path,
};

use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    crs::{enu_to_ecef_transform, project_to_wgs84},
    geometry::{Bounds, Mesh, Vec3},
    glb::build_glb_with_extras,
    inspect_drilldown::{ApprovalManifest, ApprovalManifests},
    project::ProjectManifest,
};

const LINE_THICKNESS_M: f64 = 0.18;
const SURFACE_MIN_THICKNESS_M: f64 = 0.35;

#[derive(Debug, Clone, PartialEq)]
pub struct GeometryPreviewFeature {
    pub feature_id: i64,
    pub source_id: String,
    pub layer: String,
    pub geometry_type: String,
    pub bbox: [f64; 6],
}

#[derive(Debug, Clone)]
pub struct GeometryPreviewBuildOutput {
    pub glb: Vec<u8>,
    pub mesh: Mesh,
    pub report: GeometryPublishReport,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeometryPublishReport {
    pub generated_at: String,
    pub preview_version: u32,
    pub source_count: usize,
    pub approved_source_ids: Vec<String>,
    pub feature_count: usize,
    pub line_feature_count: usize,
    pub surface_feature_count: usize,
    pub fallback_feature_count: usize,
    pub vertex_count: usize,
    pub triangle_count: usize,
    pub origin_epsg3826: [f64; 3],
    pub origin_wgs84: [f64; 3],
    pub model_matrix: [f64; 16],
    pub local_bbox: [f64; 6],
    pub raw_glb_path: String,
    pub tile_glb_path: String,
    pub tileset_path: String,
    pub geometry_file_size: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewKind {
    Line,
    Surface,
    Fallback,
}

pub fn build_minimal_geometry_preview(
    source_id: &str,
    origin_epsg3826: [f64; 3],
    origin_wgs84: [f64; 3],
    model_matrix: [f64; 16],
    features: &[GeometryPreviewFeature],
) -> Result<GeometryPreviewBuildOutput> {
    ensure!(!features.is_empty(), "geometry preview has no features");
    ensure!(
        features.len() <= u16::MAX as usize,
        "geometry preview has {} features; split preview is required before u16 batch ids",
        features.len()
    );

    let mut mesh = Mesh::new();
    let mut source_ids = BTreeSet::new();
    let mut line_feature_count = 0;
    let mut surface_feature_count = 0;
    let mut fallback_feature_count = 0;

    for (batch_id, feature) in features.iter().enumerate() {
        if source_id != "*" && feature.source_id != source_id {
            bail!(
                "geometry preview feature source mismatch: expected {source_id}, got {}",
                feature.source_id
            );
        }
        source_ids.insert(feature.source_id.clone());
        match preview_kind(feature) {
            PreviewKind::Line => {
                line_feature_count += 1;
                append_preview_box(
                    &mut mesh,
                    expand_bbox(feature.bbox, LINE_THICKNESS_M)?,
                    origin_epsg3826,
                    batch_id as u16,
                    [1.0, 0.72, 0.20, 1.0],
                );
            }
            PreviewKind::Surface => {
                surface_feature_count += 1;
                append_preview_box(
                    &mut mesh,
                    expand_bbox(feature.bbox, SURFACE_MIN_THICKNESS_M)?,
                    origin_epsg3826,
                    batch_id as u16,
                    [0.35, 0.78, 1.0, 1.0],
                );
            }
            PreviewKind::Fallback => {
                fallback_feature_count += 1;
                append_preview_box(
                    &mut mesh,
                    expand_bbox(feature.bbox, SURFACE_MIN_THICKNESS_M)?,
                    origin_epsg3826,
                    batch_id as u16,
                    [0.72, 0.72, 0.72, 1.0],
                );
            }
        }
    }

    ensure!(!mesh.is_empty(), "geometry preview mesh is empty");
    let local_bbox = bounds_to_bbox(mesh.bounds);
    let approved_source_ids: Vec<_> = source_ids.into_iter().collect();
    let extras = json!({
        "preview": "minimal_geometry_preview",
        "feature_count": features.len(),
        "source_ids": approved_source_ids.clone(),
        "note": "Entity bbox proxy preview; not production geometry fidelity"
    });
    let glb = build_glb_with_extras(&mesh, Some(extras))?;
    let report = GeometryPublishReport {
        generated_at: chrono_like_now(),
        preview_version: 1,
        source_count: approved_source_ids.len(),
        approved_source_ids,
        feature_count: features.len(),
        line_feature_count,
        surface_feature_count,
        fallback_feature_count,
        vertex_count: mesh.positions.len(),
        triangle_count: mesh.triangle_count(),
        origin_epsg3826,
        origin_wgs84,
        model_matrix,
        local_bbox,
        raw_glb_path: "raw.glb".to_string(),
        tile_glb_path: "tile.glb".to_string(),
        tileset_path: "tileset.json".to_string(),
        geometry_file_size: glb.len() as u64,
        warnings: vec![
            "minimal preview uses entity bbox proxy geometry, not final CAD mesh fidelity"
                .to_string(),
        ],
    };

    Ok(GeometryPreviewBuildOutput { glb, mesh, report })
}

pub fn build_geometry_preview_tileset_json(model_matrix: [f64; 16], local_bbox: [f64; 6]) -> Value {
    let cx = (local_bbox[0] + local_bbox[3]) * 0.5;
    let cy = (local_bbox[1] + local_bbox[4]) * 0.5;
    let cz = (local_bbox[2] + local_bbox[5]) * 0.5;
    let hx = ((local_bbox[3] - local_bbox[0]).abs() * 0.5).max(0.5);
    let hy = ((local_bbox[4] - local_bbox[1]).abs() * 0.5).max(0.5);
    let hz = ((local_bbox[5] - local_bbox[2]).abs() * 0.5).max(0.5);
    json!({
        "asset": {
            "version": "1.1",
            "generator": "ifc_to_3dtiles Phase 1K minimal geometry preview"
        },
        "geometricError": 0,
        "root": {
            "boundingVolume": {
                "box": [cx, cy, cz, hx, 0, 0, 0, hy, 0, 0, 0, hz]
            },
            "geometricError": 0,
            "refine": "ADD",
            "transform": model_matrix,
            "content": {
                "uri": "tile.glb"
            }
        }
    })
}

pub fn write_geometry_preview_outputs(input: &Path, output: &Path) -> Result<()> {
    let preview_dir = output.join("geometry_preview");
    fs::create_dir_all(&preview_dir)
        .with_context(|| format!("建立 geometry preview 目錄失敗：{}", preview_dir.display()))?;

    let manifest: ProjectManifest = read_json(&input.join("source_manifest.json"))?;
    let approvals = read_approval_manifests(&input.join("qa"))?;
    let conn = Connection::open(input.join("project_inspect.db"))
        .with_context(|| "開啟 project_inspect.db 失敗".to_string())?;
    let source_map: HashMap<_, _> = manifest
        .sources
        .iter()
        .map(|source| (source.id.clone(), source))
        .collect();

    let mut approved_source_ids = Vec::new();
    let mut features = Vec::new();
    let mut source_bboxes = Vec::new();
    for approval in &approvals.approved.sources {
        let source = source_map.get(&approval.source_id).with_context(|| {
            format!(
                "approved source 不在 source_manifest：{}",
                approval.source_id
            )
        })?;
        approved_source_ids.push(approval.source_id.clone());
        if let Some(bbox) = read_source_percentile_bbox(&conn, &approval.source_id)?
            .or(source.percentile_bbox)
            .or(source.raw_bbox)
        {
            source_bboxes.push(bbox);
        }
        features.extend(read_geometry_preview_features(&conn, &approval.source_id)?);
    }
    ensure!(
        !approved_source_ids.is_empty(),
        "沒有 approved source 可產生 preview"
    );
    ensure!(
        !features.is_empty(),
        "approved source 沒有 entity bbox 可產生 preview"
    );

    let merged_bbox = merge_bboxes(&source_bboxes).unwrap_or_else(|| {
        merge_bboxes(
            &features
                .iter()
                .map(|feature| feature.bbox)
                .collect::<Vec<_>>(),
        )
        .unwrap_or([0.0; 6])
    });
    let origin_epsg3826 = bbox_center(merged_bbox);
    let lonlat = project_to_wgs84(manifest.source_epsg, origin_epsg3826[0], origin_epsg3826[1])?;
    let origin_wgs84 = [lonlat.lon_deg, lonlat.lat_deg, origin_epsg3826[2]];
    let model_matrix = enu_to_ecef_transform(lonlat.lon_deg, lonlat.lat_deg, origin_epsg3826[2]);
    let preview = build_minimal_geometry_preview(
        "*",
        origin_epsg3826,
        origin_wgs84,
        model_matrix,
        &features,
    )?;

    fs::write(preview_dir.join("raw.glb"), &preview.glb)
        .with_context(|| "寫入 raw.glb 失敗".to_string())?;
    fs::write(preview_dir.join("tile.glb"), &preview.glb)
        .with_context(|| "寫入 tile.glb 失敗".to_string())?;
    fs::write(
        preview_dir.join("tileset.json"),
        serde_json::to_vec_pretty(&build_geometry_preview_tileset_json(
            preview.report.model_matrix,
            preview.report.local_bbox,
        ))?,
    )
    .with_context(|| "寫入 geometry preview tileset.json 失敗".to_string())?;
    fs::write(
        preview_dir.join("geometry_publish_report.json"),
        serde_json::to_vec_pretty(&preview.report)?,
    )
    .with_context(|| "寫入 geometry_publish_report.json 失敗".to_string())?;

    Ok(())
}

fn preview_kind(feature: &GeometryPreviewFeature) -> PreviewKind {
    let kind = feature.geometry_type.to_ascii_uppercase();
    if kind.contains("LINE") || kind.contains("CURVE") {
        PreviewKind::Line
    } else if kind.contains("POLYGON")
        || kind.contains("POLYHEDRALSURFACE")
        || kind.contains("TRIANGLE")
        || kind.contains("TIN")
        || kind.contains("GEOMETRYCOLLECTION")
    {
        PreviewKind::Surface
    } else {
        PreviewKind::Fallback
    }
}

fn expand_bbox(bbox: [f64; 6], min_thickness: f64) -> Result<[f64; 6]> {
    ensure!(
        bbox.iter().all(|value| value.is_finite()),
        "preview bbox contains non-finite coordinate"
    );
    let mut out = bbox;
    for axis in 0..3 {
        if out[axis + 3] < out[axis] {
            out.swap(axis, axis + 3);
        }
        let size = out[axis + 3] - out[axis];
        if size < min_thickness {
            let center = (out[axis] + out[axis + 3]) * 0.5;
            out[axis] = center - min_thickness * 0.5;
            out[axis + 3] = center + min_thickness * 0.5;
        }
    }
    Ok(out)
}

fn append_preview_box(
    mesh: &mut Mesh,
    bbox: [f64; 6],
    origin: [f64; 3],
    batch_id: u16,
    color: [f32; 4],
) {
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
            color,
        );
        append_triangle(
            mesh,
            corners[indices[0]],
            corners[indices[2]],
            corners[indices[3]],
            normal,
            batch_id,
            color,
        );
    }
}

fn append_triangle(
    mesh: &mut Mesh,
    a: [f64; 3],
    b: [f64; 3],
    c: [f64; 3],
    normal: [f64; 3],
    batch_id: u16,
    color: [f32; 4],
) {
    for point in [a, b, c] {
        mesh.positions.push(point);
        mesh.normals.push(normal);
        mesh.colors.push(color);
        mesh.batch_ids.push(batch_id);
        mesh.bounds.include(Vec3::new(point[0], point[1], point[2]));
    }
}

fn local_point(point: [f64; 3], origin: [f64; 3]) -> [f64; 3] {
    [
        point[0] - origin[0],
        point[1] - origin[1],
        point[2] - origin[2],
    ]
}

fn read_geometry_preview_features(
    conn: &Connection,
    source_id: &str,
) -> Result<Vec<GeometryPreviewFeature>> {
    let mut stmt = conn.prepare(
        "SELECT e.fid, e.source_id, COALESCE(e.layer, ''), COALESCE(e.geometry_type, ''),
                b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z
         FROM entities e
         JOIN entity_bboxes b ON b.source_id = e.source_id AND b.fid = e.fid
         WHERE e.source_id = ?1
         ORDER BY e.fid",
    )?;
    let rows = stmt.query_map(params![source_id], |row| {
        Ok(GeometryPreviewFeature {
            feature_id: row.get(0)?,
            source_id: row.get(1)?,
            layer: row.get(2)?,
            geometry_type: row.get(3)?,
            bbox: [
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
            ],
        })
    })?;
    let mut features = Vec::new();
    for row in rows {
        features.push(row?);
    }
    Ok(features)
}

fn read_source_percentile_bbox(conn: &Connection, source_id: &str) -> Result<Option<[f64; 6]>> {
    let text: Option<String> = conn
        .query_row(
            "SELECT percentile_bbox_json FROM source_stats WHERE source_id = ?1",
            params![source_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(text) = text else {
        return Ok(None);
    };
    if text.trim().is_empty() || text.trim() == "null" {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&text)?))
}

fn read_approval_manifests(path: &Path) -> Result<ApprovalManifests> {
    Ok(ApprovalManifests {
        approved: read_approval_manifest(&path.join("approved_sources.json"))?,
        rejected: read_approval_manifest(&path.join("rejected_sources.json"))?,
        needs_review: read_approval_manifest(&path.join("needs_review_sources.json"))?,
    })
}

fn read_approval_manifest(path: &Path) -> Result<ApprovalManifest> {
    read_json(path)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    serde_json::from_slice(
        &fs::read(path).with_context(|| format!("讀取 JSON 失敗：{}", path.display()))?,
    )
    .with_context(|| format!("解析 JSON 失敗：{}", path.display()))
}

fn bbox_center(bbox: [f64; 6]) -> [f64; 3] {
    [
        (bbox[0] + bbox[3]) * 0.5,
        (bbox[1] + bbox[4]) * 0.5,
        (bbox[2] + bbox[5]) * 0.5,
    ]
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

fn bounds_to_bbox(bounds: Bounds) -> [f64; 6] {
    [
        bounds.min.x,
        bounds.min.y,
        bounds.min.z,
        bounds.max.x,
        bounds.max.y,
        bounds.max.z,
    ]
}

fn chrono_like_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| format!("unix:{}", duration.as_secs()))
        .unwrap_or_else(|_| "unix:0".to_string())
}

trait OptionalRow<T> {
    fn optional(self) -> rusqlite::Result<Option<T>>;
}

impl<T> OptionalRow<T> for rusqlite::Result<T> {
    fn optional(self) -> rusqlite::Result<Option<T>> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(err),
        }
    }
}
