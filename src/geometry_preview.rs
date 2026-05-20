use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
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
const LINE_WIDTH_EXAGGERATION: f64 = 4.0;
const TINY_LINE_LENGTH_M: f64 = 0.5;
const TINY_SURFACE_AREA_M2: f64 = 0.05;
const DEBUG_MARKER_SIZE_M: f64 = 0.6;
const DIAGNOSTIC_CENTER_TOLERANCE_M: f64 = 0.5;
const DIAGNOSTIC_SIZE_RATIO_TOLERANCE: f64 = 10.0;
const DIAGNOSTIC_SOURCE_MARGIN_M: f64 = 50.0;
const DIAGNOSTIC_TINY_AXIS_M: f64 = 0.18;
const DIAGNOSTIC_HIGH_TRIANGLE_DENSITY: f64 = 10_000.0;

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
    pub skipped_tiny_feature_count: usize,
    pub debug_marker_count: usize,
    pub degenerate_skipped_count: usize,
    pub debug_inflated_feature_count: usize,
    pub visual_category_counts: BTreeMap<String, usize>,
    pub line_width_exaggeration: f64,
    pub surface_shading_mode: String,
    pub double_side_debug_available: bool,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeometryDiagnosticReport {
    pub generated_at: String,
    pub diagnostic_version: u32,
    pub source_count: usize,
    pub feature_count: usize,
    pub bad_feature_count: usize,
    pub bbox_mismatch_count: usize,
    pub outlier_geometry_count: usize,
    pub category_counts: BTreeMap<String, usize>,
    pub tolerances: GeometryDiagnosticTolerances,
    pub source_bbox: [f64; 6],
    pub source_bbox_wgs84: Option<[f64; 6]>,
    pub origin_epsg3826: [f64; 3],
    pub origin_wgs84: [f64; 3],
    pub model_matrix_finite: bool,
    pub features: Vec<GeometryDiagnosticFeature>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeometryDiagnosticTolerances {
    pub center_distance_m: f64,
    pub size_ratio: f64,
    pub source_margin_m: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeometryDiagnosticFeature {
    pub feature_id: i64,
    pub source_id: String,
    pub layer: String,
    pub category: String,
    pub cleanup_action: String,
    pub mesh_exported: bool,
    pub vertex_count: usize,
    pub triangle_count: usize,
    pub bbox: [f64; 6],
    pub pick_bbox: [f64; 6],
    pub bbox_wgs84: Option<[f64; 6]>,
    pub pick_bbox_wgs84: Option<[f64; 6]>,
    pub center: [f64; 3],
    pub center_wgs84: Option<[f64; 3]>,
    pub size: [f64; 3],
    pub diagonal_length: f64,
    #[serde(rename = "hasNaN")]
    pub has_nan: bool,
    pub has_infinite: bool,
    pub has_degenerate_triangles: bool,
    pub degenerate_triangle_count: usize,
    pub zero_area_triangle_count: usize,
    pub duplicate_vertex_ratio: f64,
    pub normal_status: String,
    pub transform_status: String,
    pub bbox_center_distance: f64,
    pub bbox_size_ratio: f64,
    pub bbox_overlap_ratio: f64,
    pub bbox_tolerance_exceeded: bool,
    pub mismatch_level: String,
    pub outlier_geometry: bool,
    pub distance_from_scene_center: f64,
    pub size_percentile: f64,
    pub triangle_density: f64,
    pub abnormal_aspect_ratio: f64,
    pub severity_score: f64,
    pub problem_category: String,
    pub problem_flags: Vec<String>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeometryTransformDiffReport {
    pub report_type: String,
    pub generated_at: String,
    pub report_version: u32,
    pub transform_diff_feature_count: usize,
    pub mismatch_feature_count: usize,
    pub far_away_feature_count: usize,
    pub possible_cause_histogram: BTreeMap<String, usize>,
    pub far_away_feature_ids: Vec<i64>,
    pub features: Vec<GeometryTransformDiffFeature>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeometryTransformDiffFeature {
    pub feature_id: i64,
    pub source: String,
    pub source_id: String,
    pub layer: String,
    pub category: String,
    #[serde(rename = "geometryBBox")]
    pub geometry_bbox: [f64; 6],
    #[serde(rename = "pickBBox")]
    pub pick_bbox: [f64; 6],
    pub geometry_center: [f64; 3],
    pub pick_center: [f64; 3],
    pub center_delta: [f64; 3],
    pub center_distance: f64,
    pub geometry_size: [f64; 3],
    pub pick_size: [f64; 3],
    #[serde(rename = "sizeRatioXYZ")]
    pub size_ratio_xyz: [f64; 3],
    pub diagonal_ratio: f64,
    pub overlap_ratio: f64,
    pub possible_cause: String,
    pub distance_from_scene_center: Option<f64>,
    pub nearest_normal_feature_distance: Option<f64>,
    pub source_offset_candidate: Option<[f64; 3]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewKind {
    Line,
    Surface,
    Fallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewCleanupAction {
    Skip,
    KeepAsPointMarker,
    InflateForDebugOnly,
    KeepRaw,
}

impl PreviewCleanupAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::KeepAsPointMarker => "keep_as_point_marker",
            Self::InflateForDebugOnly => "inflate_for_debug_only",
            Self::KeepRaw => "keep_raw",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum VisualCategory {
    Wall,
    Slab,
    Beam,
    Column,
    Annotation,
    Linework,
    Marker,
    Unknown,
}

impl VisualCategory {
    fn as_str(self) -> &'static str {
        match self {
            Self::Wall => "wall",
            Self::Slab => "slab",
            Self::Beam => "beam",
            Self::Column => "column",
            Self::Annotation => "annotation",
            Self::Linework => "linework",
            Self::Marker => "marker",
            Self::Unknown => "unknown",
        }
    }
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
    let mut skipped_tiny_feature_count = 0;
    let mut debug_marker_count = 0;
    let mut degenerate_skipped_count = 0;
    let mut debug_inflated_feature_count = 0;
    let mut visual_category_counts = BTreeMap::new();

    for (batch_id, feature) in features.iter().enumerate() {
        if source_id != "*" && feature.source_id != source_id {
            bail!(
                "geometry preview feature source mismatch: expected {source_id}, got {}",
                feature.source_id
            );
        }
        source_ids.insert(feature.source_id.clone());
        let kind = preview_kind(feature);
        let cleanup_action = preview_cleanup_action(feature);
        let visual_category = visual_category(feature, kind, cleanup_action);
        *visual_category_counts
            .entry(visual_category.as_str().to_string())
            .or_insert(0) += 1;
        match kind {
            PreviewKind::Line => {
                line_feature_count += 1;
            }
            PreviewKind::Surface => {
                surface_feature_count += 1;
            }
            PreviewKind::Fallback => {
                fallback_feature_count += 1;
            }
        }

        let Some(bbox) = effective_preview_bbox(feature, cleanup_action)? else {
            skipped_tiny_feature_count += 1;
            degenerate_skipped_count += 1;
            continue;
        };
        let color = match cleanup_action {
            PreviewCleanupAction::Skip => unreachable!("skip already returned None"),
            PreviewCleanupAction::KeepAsPointMarker => {
                debug_marker_count += 1;
                visual_category_color(visual_category)
            }
            PreviewCleanupAction::InflateForDebugOnly => {
                debug_inflated_feature_count += 1;
                visual_category_color(visual_category)
            }
            PreviewCleanupAction::KeepRaw => visual_category_color(visual_category),
        };
        append_preview_box(&mut mesh, bbox, origin_epsg3826, batch_id as u16, color);
    }

    ensure!(!mesh.is_empty(), "geometry preview mesh is empty");
    let local_bbox = bounds_to_bbox(mesh.bounds);
    let approved_source_ids: Vec<_> = source_ids.into_iter().collect();
    let extras = json!({
        "preview": "minimal_geometry_preview",
        "feature_count": features.len(),
        "source_ids": approved_source_ids.clone(),
        "visual_category_counts": visual_category_counts.clone(),
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
        skipped_tiny_feature_count,
        debug_marker_count,
        degenerate_skipped_count,
        debug_inflated_feature_count,
        visual_category_counts,
        line_width_exaggeration: LINE_WIDTH_EXAGGERATION,
        surface_shading_mode: "category_color_with_normals".to_string(),
        double_side_debug_available: true,
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

pub fn build_geometry_diagnostic_report(
    source_epsg: u32,
    origin_epsg3826: [f64; 3],
    origin_wgs84: [f64; 3],
    model_matrix: [f64; 16],
    source_bbox: [f64; 6],
    features: &[GeometryPreviewFeature],
) -> Result<GeometryDiagnosticReport> {
    ensure!(
        !features.is_empty(),
        "geometry diagnostic report has no features"
    );

    let model_matrix_finite = model_matrix.iter().all(|value| value.is_finite());
    let mut category_counts = BTreeMap::new();
    let mut diagnostic_features = Vec::with_capacity(features.len());
    let mut bad_feature_count = 0;
    let mut bbox_mismatch_count = 0;
    let mut outlier_geometry_count = 0;
    let mut source_ids = BTreeSet::new();
    let source_bbox = normalize_bbox(source_bbox);

    for feature in features {
        source_ids.insert(feature.source_id.clone());
        let diagnostic = diagnose_geometry_feature(
            source_epsg,
            origin_epsg3826,
            model_matrix_finite,
            source_bbox,
            feature,
        )?;
        if diagnostic.problem_category != "none" {
            bad_feature_count += 1;
        }
        if diagnostic.bbox_tolerance_exceeded {
            bbox_mismatch_count += 1;
        }
        if diagnostic.outlier_geometry {
            outlier_geometry_count += 1;
        }
        *category_counts
            .entry(diagnostic.problem_category.clone())
            .or_insert(0) += 1;
        diagnostic_features.push(diagnostic);
    }
    assign_size_percentiles(&mut diagnostic_features);
    for feature in &mut diagnostic_features {
        feature.severity_score = severity_score(feature);
    }

    Ok(GeometryDiagnosticReport {
        generated_at: chrono_like_now(),
        diagnostic_version: 1,
        source_count: source_ids.len(),
        feature_count: diagnostic_features.len(),
        bad_feature_count,
        bbox_mismatch_count,
        outlier_geometry_count,
        category_counts,
        tolerances: GeometryDiagnosticTolerances {
            center_distance_m: DIAGNOSTIC_CENTER_TOLERANCE_M,
            size_ratio: DIAGNOSTIC_SIZE_RATIO_TOLERANCE,
            source_margin_m: DIAGNOSTIC_SOURCE_MARGIN_M,
        },
        source_bbox,
        source_bbox_wgs84: bbox_to_wgs84(source_epsg, source_bbox),
        origin_epsg3826,
        origin_wgs84,
        model_matrix_finite,
        features: diagnostic_features,
        warnings: vec![
            "diagnostic report classifies proxy preview geometry; it does not repair CAD/BIM geometry"
                .to_string(),
        ],
    })
}

pub fn build_geometry_transform_diff_report(
    diagnostic: &GeometryDiagnosticReport,
) -> Result<GeometryTransformDiffReport> {
    let normal_centers: Vec<[f64; 3]> = diagnostic
        .features
        .iter()
        .filter(|feature| {
            feature.problem_category == "none"
                && !feature.bbox_tolerance_exceeded
                && !feature.outlier_geometry
        })
        .map(|feature| feature.center)
        .collect();
    let source_center = bbox_center(diagnostic.source_bbox);
    let mut features = Vec::new();
    let mut possible_cause_histogram = BTreeMap::new();
    let mut far_away_feature_ids = Vec::new();
    let mut mismatch_feature_count = 0;

    for feature in &diagnostic.features {
        let include = feature.bbox_tolerance_exceeded
            || feature.outlier_geometry
            || feature.problem_flags.iter().any(|flag| {
                flag == "bbox_mismatch" || flag == "transform_mismatch" || flag == "far_away"
            });
        if !include {
            continue;
        }
        if feature.bbox_tolerance_exceeded {
            mismatch_feature_count += 1;
        }
        if feature.outlier_geometry || feature.problem_flags.iter().any(|flag| flag == "far_away") {
            far_away_feature_ids.push(feature.feature_id);
        }
        let diff = transform_diff_feature(feature, source_center, &normal_centers);
        *possible_cause_histogram
            .entry(diff.possible_cause.clone())
            .or_insert(0) += 1;
        features.push(diff);
    }

    Ok(GeometryTransformDiffReport {
        report_type: "geometryTransformDiffReport".to_string(),
        generated_at: chrono_like_now(),
        report_version: 1,
        transform_diff_feature_count: features.len(),
        mismatch_feature_count,
        far_away_feature_count: far_away_feature_ids.len(),
        possible_cause_histogram,
        far_away_feature_ids,
        features,
        warnings: vec![
            "diagnostics only; Phase 1M does not modify geometry or viewer behavior".to_string(),
        ],
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
    let diagnostic = build_geometry_diagnostic_report(
        manifest.source_epsg,
        origin_epsg3826,
        origin_wgs84,
        model_matrix,
        merged_bbox,
        &features,
    )?;
    let transform_diff = build_geometry_transform_diff_report(&diagnostic)?;

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
    fs::write(
        preview_dir.join("geometry_diagnostic_report.json"),
        serde_json::to_vec_pretty(&diagnostic)?,
    )
    .with_context(|| "寫入 geometry_diagnostic_report.json 失敗".to_string())?;
    fs::write(
        output.join("geometry_diagnostic_report.json"),
        serde_json::to_vec_pretty(&diagnostic)?,
    )
    .with_context(|| "寫入 publish root geometry_diagnostic_report.json 失敗".to_string())?;
    fs::write(
        output.join("geometry_transform_diff_report.json"),
        serde_json::to_vec_pretty(&transform_diff)?,
    )
    .with_context(|| "寫入 geometry_transform_diff_report.json 失敗".to_string())?;

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

fn preview_cleanup_action(feature: &GeometryPreviewFeature) -> PreviewCleanupAction {
    if !feature.bbox.iter().all(|value| value.is_finite()) {
        return PreviewCleanupAction::Skip;
    }
    let bbox = normalize_bbox(feature.bbox);
    let size = bbox_size(bbox);
    let diagonal = bbox_diagonal(bbox);
    let mut sorted_size = size;
    sorted_size.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let has_debug_value = has_meaningful_debug_metadata(feature);

    match preview_kind(feature) {
        PreviewKind::Line => {
            if diagonal < TINY_LINE_LENGTH_M {
                if has_debug_value {
                    PreviewCleanupAction::KeepAsPointMarker
                } else {
                    PreviewCleanupAction::Skip
                }
            } else if size.iter().any(|axis| *axis < LINE_THICKNESS_M) {
                PreviewCleanupAction::InflateForDebugOnly
            } else {
                PreviewCleanupAction::KeepRaw
            }
        }
        PreviewKind::Surface | PreviewKind::Fallback => {
            let surface_area_hint = sorted_size[1] * sorted_size[2];
            if surface_area_hint < TINY_SURFACE_AREA_M2 {
                if has_debug_value {
                    PreviewCleanupAction::KeepAsPointMarker
                } else {
                    PreviewCleanupAction::Skip
                }
            } else if size.iter().any(|axis| *axis < SURFACE_MIN_THICKNESS_M) {
                PreviewCleanupAction::InflateForDebugOnly
            } else {
                PreviewCleanupAction::KeepRaw
            }
        }
    }
}

fn has_meaningful_debug_metadata(feature: &GeometryPreviewFeature) -> bool {
    let layer = feature.layer.trim();
    !layer.is_empty() && layer != "0"
}

fn effective_preview_bbox(
    feature: &GeometryPreviewFeature,
    action: PreviewCleanupAction,
) -> Result<Option<[f64; 6]>> {
    let kind = preview_kind(feature);
    let min_thickness = match kind {
        PreviewKind::Line => LINE_THICKNESS_M,
        PreviewKind::Surface | PreviewKind::Fallback => SURFACE_MIN_THICKNESS_M,
    };
    let bbox = normalize_bbox(feature.bbox);
    match action {
        PreviewCleanupAction::Skip => Ok(None),
        PreviewCleanupAction::KeepAsPointMarker => {
            Ok(Some(marker_bbox(bbox_center(bbox), DEBUG_MARKER_SIZE_M)))
        }
        PreviewCleanupAction::InflateForDebugOnly => Ok(Some(expand_bbox(bbox, min_thickness)?)),
        PreviewCleanupAction::KeepRaw => Ok(Some(expand_bbox(bbox, min_thickness)?)),
    }
}

fn marker_bbox(center: [f64; 3], size: f64) -> [f64; 6] {
    let half = size * 0.5;
    [
        center[0] - half,
        center[1] - half,
        center[2] - half,
        center[0] + half,
        center[1] + half,
        center[2] + half,
    ]
}

fn visual_category(
    feature: &GeometryPreviewFeature,
    kind: PreviewKind,
    cleanup_action: PreviewCleanupAction,
) -> VisualCategory {
    let text = format!(
        "{} {}",
        feature.layer.to_ascii_lowercase(),
        feature.geometry_type.to_ascii_lowercase()
    );
    if contains_any(
        &text,
        &["anno", "annotation", "text", "dim", "label", "註", "文字"],
    ) {
        return VisualCategory::Annotation;
    }
    if matches!(kind, PreviewKind::Line) {
        return VisualCategory::Linework;
    }
    if contains_any(&text, &["wall", "墻", "牆"]) {
        VisualCategory::Wall
    } else if contains_any(&text, &["slab", "floor", "deck", "版"]) {
        VisualCategory::Slab
    } else if contains_any(&text, &["beam", "girder", "cable", "梁"]) {
        VisualCategory::Beam
    } else if contains_any(
        &text,
        &["column", "pier", "pile", "tower", "柱", "墩", "塔"],
    ) {
        VisualCategory::Column
    } else if cleanup_action == PreviewCleanupAction::KeepAsPointMarker {
        VisualCategory::Marker
    } else {
        VisualCategory::Unknown
    }
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn visual_category_color(category: VisualCategory) -> [f32; 4] {
    match category {
        VisualCategory::Wall => [0.35, 0.68, 1.0, 1.0],
        VisualCategory::Slab => [0.55, 0.78, 0.42, 1.0],
        VisualCategory::Beam => [1.0, 0.68, 0.22, 1.0],
        VisualCategory::Column => [0.72, 0.50, 1.0, 1.0],
        VisualCategory::Annotation => [0.78, 0.82, 0.88, 1.0],
        VisualCategory::Linework => [1.0, 0.78, 0.24, 1.0],
        VisualCategory::Marker => [1.0, 0.2, 0.75, 1.0],
        VisualCategory::Unknown => [0.68, 0.70, 0.72, 1.0],
    }
}

fn diagnose_geometry_feature(
    source_epsg: u32,
    origin_epsg3826: [f64; 3],
    model_matrix_finite: bool,
    source_bbox: [f64; 6],
    feature: &GeometryPreviewFeature,
) -> Result<GeometryDiagnosticFeature> {
    let kind = preview_kind(feature);
    let cleanup_action = preview_cleanup_action(feature);
    let pick_bbox = normalize_bbox(feature.bbox);
    let mesh_exported = cleanup_action != PreviewCleanupAction::Skip;
    let bbox = effective_preview_bbox(feature, cleanup_action)?.unwrap_or(pick_bbox);
    let center = bbox_center(bbox);
    let pick_center = bbox_center(pick_bbox);
    let size = bbox_size(bbox);
    let pick_size = bbox_size(pick_bbox);
    let has_nan = bbox
        .iter()
        .chain(pick_bbox.iter())
        .any(|value| value.is_nan());
    let has_infinite = bbox
        .iter()
        .chain(pick_bbox.iter())
        .any(|value| value.is_infinite());
    let bbox_center_distance = distance3(center, pick_center);
    let bbox_size_ratio = max_axis_size_ratio(size, pick_size);
    let bbox_overlap_ratio = bbox_overlap_ratio(bbox, pick_bbox);
    let raw_bbox_tolerance_exceeded = bbox_center_distance > DIAGNOSTIC_CENTER_TOLERANCE_M
        || bbox_size_ratio > DIAGNOSTIC_SIZE_RATIO_TOLERANCE;
    let intentional_debug_geometry = cleanup_action != PreviewCleanupAction::KeepRaw;
    let bbox_tolerance_exceeded = raw_bbox_tolerance_exceeded && !intentional_debug_geometry;
    let mismatch_level = if intentional_debug_geometry {
        "none".to_string()
    } else {
        mismatch_level(bbox_center_distance, bbox_size_ratio)
    };
    let has_degenerate_triangles = mesh_exported && size.iter().any(|value| *value <= 1e-9);
    let degenerate_triangle_count =
        if mesh_exported && (has_degenerate_triangles || has_nan || has_infinite) {
            12
        } else {
            0
        };
    let zero_area_triangle_count = degenerate_triangle_count;
    let duplicate_vertex_ratio = 0.0;
    let line_too_thin = matches!(kind, PreviewKind::Line)
        && pick_size
            .iter()
            .filter(|value| **value < LINE_THICKNESS_M)
            .count()
            >= 1;
    let raw_outlier_geometry =
        !bbox_inside_with_margin(source_bbox, pick_bbox, DIAGNOSTIC_SOURCE_MARGIN_M);
    let outlier_geometry = raw_outlier_geometry && !intentional_debug_geometry;
    let source_center = bbox_center(source_bbox);
    let source_diagonal = bbox_diagonal(source_bbox).max(1.0);
    let diagonal_length = bbox_diagonal(bbox);
    let distance_from_scene_center = distance3(center, source_center);
    let volume = bbox_volume(bbox).max(0.001);
    let triangle_density = 12.0 / volume;
    let abnormal_aspect_ratio = aspect_ratio(size);
    let local_center = local_point(center, origin_epsg3826);
    let transform_status = if has_nan || has_infinite || !model_matrix_finite {
        "non_finite".to_string()
    } else if local_center.iter().any(|value| value.abs() > 100_000.0) {
        "large_local_coordinates".to_string()
    } else {
        "ok".to_string()
    };
    let normal_status = if has_degenerate_triangles || has_nan || has_infinite {
        "degenerate".to_string()
    } else {
        "ok".to_string()
    };

    let mut reasons = Vec::new();
    if has_nan {
        reasons.push("coordinate contains NaN value".to_string());
    }
    if has_infinite {
        reasons.push("coordinate contains infinite value".to_string());
    }
    if !model_matrix_finite {
        reasons.push("model matrix contains non-finite value".to_string());
    }
    if has_degenerate_triangles {
        reasons.push("expanded proxy bbox still has degenerate triangles".to_string());
    }
    match cleanup_action {
        PreviewCleanupAction::Skip => {
            reasons.push("tiny or degenerate feature skipped from preview mesh".to_string());
        }
        PreviewCleanupAction::KeepAsPointMarker => {
            reasons.push("tiny feature emitted as debug point marker".to_string());
        }
        PreviewCleanupAction::InflateForDebugOnly => {
            reasons.push("near-zero feature intentionally inflated for debug preview".to_string());
        }
        PreviewCleanupAction::KeepRaw => {}
    }
    if line_too_thin {
        reasons.push("source line/polyline bbox has zero or tiny thickness".to_string());
    }
    if bbox_tolerance_exceeded {
        reasons.push(format!(
            "geometry bbox differs from pick bbox beyond tolerance: center={:.3}m ratio={:.3}",
            bbox_center_distance, bbox_size_ratio
        ));
    }
    if outlier_geometry {
        reasons.push("feature bbox is outside approved source bbox margin".to_string());
    }
    if abnormal_aspect_ratio > 100.0 {
        reasons.push(format!(
            "feature bbox has abnormal aspect ratio {:.3}",
            abnormal_aspect_ratio
        ));
    }
    if triangle_density > DIAGNOSTIC_HIGH_TRIANGLE_DENSITY {
        reasons.push(format!(
            "feature triangle density is high: {:.3} triangles/m3",
            triangle_density
        ));
    }

    let problem_flags = if intentional_debug_geometry {
        cleanup_problem_flags(cleanup_action)
    } else {
        problem_flags(
            has_nan,
            has_infinite,
            degenerate_triangle_count,
            line_too_thin,
            bbox_tolerance_exceeded,
            outlier_geometry,
            transform_status.as_str(),
            diagonal_length,
            source_diagonal,
            distance_from_scene_center,
            triangle_density,
            abnormal_aspect_ratio,
        )
    };

    let problem_category = if intentional_debug_geometry {
        "none"
    } else if has_nan || has_infinite {
        "coordinate"
    } else if has_degenerate_triangles {
        "face"
    } else if transform_status != "ok" || (bbox_tolerance_exceeded && !line_too_thin) {
        "transform"
    } else if line_too_thin {
        "line"
    } else if outlier_geometry {
        "source"
    } else {
        "none"
    }
    .to_string();

    Ok(GeometryDiagnosticFeature {
        feature_id: feature.feature_id,
        source_id: feature.source_id.clone(),
        layer: feature.layer.clone(),
        category: feature.geometry_type.clone(),
        cleanup_action: cleanup_action.as_str().to_string(),
        mesh_exported,
        vertex_count: if mesh_exported { 36 } else { 0 },
        triangle_count: if mesh_exported { 12 } else { 0 },
        bbox,
        pick_bbox,
        bbox_wgs84: bbox_to_wgs84(source_epsg, bbox),
        pick_bbox_wgs84: bbox_to_wgs84(source_epsg, pick_bbox),
        center,
        center_wgs84: point_to_wgs84(source_epsg, center),
        size,
        diagonal_length,
        has_nan,
        has_infinite,
        has_degenerate_triangles,
        degenerate_triangle_count,
        zero_area_triangle_count,
        duplicate_vertex_ratio,
        normal_status,
        transform_status,
        bbox_center_distance,
        bbox_size_ratio,
        bbox_overlap_ratio,
        bbox_tolerance_exceeded,
        mismatch_level,
        outlier_geometry,
        distance_from_scene_center,
        size_percentile: 0.0,
        triangle_density,
        abnormal_aspect_ratio,
        severity_score: 0.0,
        problem_category,
        problem_flags,
        reasons,
    })
}

fn normalize_bbox(mut bbox: [f64; 6]) -> [f64; 6] {
    for axis in 0..3 {
        if bbox[axis + 3] < bbox[axis] {
            bbox.swap(axis, axis + 3);
        }
    }
    bbox
}

fn bbox_size(bbox: [f64; 6]) -> [f64; 3] {
    [
        (bbox[3] - bbox[0]).abs(),
        (bbox[4] - bbox[1]).abs(),
        (bbox[5] - bbox[2]).abs(),
    ]
}

fn bbox_diagonal(bbox: [f64; 6]) -> f64 {
    distance3([bbox[0], bbox[1], bbox[2]], [bbox[3], bbox[4], bbox[5]])
}

fn bbox_volume(bbox: [f64; 6]) -> f64 {
    let size = bbox_size(bbox);
    size[0].max(0.0) * size[1].max(0.0) * size[2].max(0.0)
}

fn bbox_overlap_ratio(a: [f64; 6], b: [f64; 6]) -> f64 {
    let mut overlap_volume = 1.0;
    for axis in 0..3 {
        let min = a[axis].max(b[axis]);
        let max = a[axis + 3].min(b[axis + 3]);
        let overlap = if max > min {
            max - min
        } else if (max - min).abs() <= 1e-9
            && b[axis] >= a[axis] - 1e-9
            && b[axis] <= a[axis + 3] + 1e-9
        {
            0.001
        } else {
            0.0
        };
        overlap_volume *= overlap;
    }
    let union_volume = bbox_volume(a).max(0.001) + bbox_volume(b).max(0.001) - overlap_volume;
    if union_volume <= 0.0 {
        0.0
    } else {
        (overlap_volume / union_volume).clamp(0.0, 1.0)
    }
}

fn aspect_ratio(size: [f64; 3]) -> f64 {
    let max_size = size.iter().copied().fold(0.0_f64, f64::max).max(0.001);
    let min_size = size
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min)
        .max(0.001);
    max_size / min_size
}

fn distance3(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn max_axis_size_ratio(a: [f64; 3], b: [f64; 3]) -> f64 {
    let mut max_ratio: f64 = 1.0;
    for axis in 0..3 {
        let left = a[axis].abs().max(0.001);
        let right = b[axis].abs().max(0.001);
        max_ratio = max_ratio.max(left / right).max(right / left);
    }
    max_ratio
}

fn mismatch_level(center_distance: f64, size_ratio: f64) -> String {
    if center_distance > 10.0 || size_ratio > 100.0 {
        "critical"
    } else if center_distance > 2.0 || size_ratio > 25.0 {
        "high"
    } else if center_distance > DIAGNOSTIC_CENTER_TOLERANCE_M
        || size_ratio > DIAGNOSTIC_SIZE_RATIO_TOLERANCE
    {
        "medium"
    } else {
        "none"
    }
    .to_string()
}

fn problem_flags(
    has_nan: bool,
    has_infinite: bool,
    degenerate_triangle_count: usize,
    line_too_thin: bool,
    bbox_tolerance_exceeded: bool,
    outlier_geometry: bool,
    transform_status: &str,
    diagonal_length: f64,
    source_diagonal: f64,
    distance_from_scene_center: f64,
    triangle_density: f64,
    abnormal_aspect_ratio: f64,
) -> Vec<String> {
    let mut flags = Vec::new();
    if has_nan {
        flags.push("nan".to_string());
    }
    if has_infinite {
        flags.push("infinite".to_string());
    }
    if degenerate_triangle_count > 0 {
        flags.push("degenerate".to_string());
    }
    if line_too_thin || diagonal_length < DIAGNOSTIC_TINY_AXIS_M {
        flags.push("tiny_bbox".to_string());
    }
    if diagonal_length > source_diagonal * 0.5 || diagonal_length > 500.0 {
        flags.push("huge_bbox".to_string());
    }
    if bbox_tolerance_exceeded {
        flags.push("bbox_mismatch".to_string());
        flags.push("transform_mismatch".to_string());
    }
    if outlier_geometry || distance_from_scene_center > source_diagonal * 0.75 {
        flags.push("far_away".to_string());
    }
    if transform_status != "ok" {
        flags.push("transform_mismatch".to_string());
    }
    if triangle_density > DIAGNOSTIC_HIGH_TRIANGLE_DENSITY {
        flags.push("high_triangle_density".to_string());
    }
    if abnormal_aspect_ratio > 100.0 {
        flags.push("abnormal_aspect_ratio".to_string());
    }
    flags.sort();
    flags.dedup();
    flags
}

fn cleanup_problem_flags(action: PreviewCleanupAction) -> Vec<String> {
    match action {
        PreviewCleanupAction::Skip => vec!["skipped_degenerate".to_string()],
        PreviewCleanupAction::KeepAsPointMarker => vec!["debug_marker".to_string()],
        PreviewCleanupAction::InflateForDebugOnly => vec!["debug_inflated".to_string()],
        PreviewCleanupAction::KeepRaw => Vec::new(),
    }
}

fn severity_score(feature: &GeometryDiagnosticFeature) -> f64 {
    let mut score: f64 = 0.0;
    if feature.has_nan || feature.has_infinite {
        score += 100.0;
    }
    if feature.degenerate_triangle_count > 0 {
        score += 60.0;
    }
    if feature.mismatch_level == "critical" {
        score += 55.0;
    } else if feature.mismatch_level == "high" {
        score += 40.0;
    } else if feature.mismatch_level == "medium" {
        score += 25.0;
    }
    if feature.outlier_geometry {
        score += 45.0;
    }
    if feature.problem_flags.iter().any(|flag| flag == "tiny_bbox") {
        score += 20.0;
    }
    if feature.problem_flags.iter().any(|flag| flag == "huge_bbox") {
        score += 35.0;
    }
    if feature.triangle_density > DIAGNOSTIC_HIGH_TRIANGLE_DENSITY {
        score += 35.0;
    }
    if feature.abnormal_aspect_ratio > 100.0 {
        score += 20.0;
    }
    score.clamp(0.0, 100.0)
}

fn assign_size_percentiles(features: &mut [GeometryDiagnosticFeature]) {
    if features.is_empty() {
        return;
    }
    let mut diagonals: Vec<f64> = features
        .iter()
        .map(|feature| feature.diagonal_length)
        .filter(|value| value.is_finite())
        .collect();
    diagonals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let denom = (diagonals.len().saturating_sub(1)).max(1) as f64;
    for feature in features {
        let rank = diagonals
            .iter()
            .position(|value| *value >= feature.diagonal_length)
            .unwrap_or(diagonals.len().saturating_sub(1));
        feature.size_percentile = (rank as f64 / denom * 100.0).clamp(0.0, 100.0);
    }
}

fn transform_diff_feature(
    feature: &GeometryDiagnosticFeature,
    source_center: [f64; 3],
    normal_centers: &[[f64; 3]],
) -> GeometryTransformDiffFeature {
    let geometry_center = feature.center;
    let pick_center = bbox_center(feature.pick_bbox);
    let center_delta = [
        geometry_center[0] - pick_center[0],
        geometry_center[1] - pick_center[1],
        geometry_center[2] - pick_center[2],
    ];
    let geometry_size = bbox_size(feature.bbox);
    let pick_size = bbox_size(feature.pick_bbox);
    let size_ratio_xyz = [
        safe_ratio(geometry_size[0], pick_size[0]),
        safe_ratio(geometry_size[1], pick_size[1]),
        safe_ratio(geometry_size[2], pick_size[2]),
    ];
    let diagonal_ratio = safe_ratio(
        bbox_diagonal(feature.bbox),
        bbox_diagonal(feature.pick_bbox),
    );
    let nearest_normal_feature_distance = nearest_center_distance(pick_center, normal_centers);
    let source_offset_candidate = if feature.outlier_geometry
        || feature.problem_flags.iter().any(|flag| flag == "far_away")
    {
        if let Some(nearest) = nearest_center(pick_center, normal_centers) {
            Some([
                pick_center[0] - nearest[0],
                pick_center[1] - nearest[1],
                pick_center[2] - nearest[2],
            ])
        } else {
            Some([
                pick_center[0] - source_center[0],
                pick_center[1] - source_center[1],
                pick_center[2] - source_center[2],
            ])
        }
    } else {
        None
    };
    let possible_cause = classify_transform_diff_cause(
        feature,
        center_delta,
        feature.bbox_center_distance,
        geometry_size,
        pick_size,
        size_ratio_xyz,
        diagonal_ratio,
    );

    GeometryTransformDiffFeature {
        feature_id: feature.feature_id,
        source: feature.source_id.clone(),
        source_id: feature.source_id.clone(),
        layer: feature.layer.clone(),
        category: feature.category.clone(),
        geometry_bbox: feature.bbox,
        pick_bbox: feature.pick_bbox,
        geometry_center,
        pick_center,
        center_delta,
        center_distance: feature.bbox_center_distance,
        geometry_size,
        pick_size,
        size_ratio_xyz,
        diagonal_ratio,
        overlap_ratio: feature.bbox_overlap_ratio,
        possible_cause,
        distance_from_scene_center: Some(feature.distance_from_scene_center),
        nearest_normal_feature_distance,
        source_offset_candidate,
    }
}

fn classify_transform_diff_cause(
    feature: &GeometryDiagnosticFeature,
    center_delta: [f64; 3],
    center_distance: f64,
    geometry_size: [f64; 3],
    pick_size: [f64; 3],
    size_ratio_xyz: [f64; 3],
    diagonal_ratio: f64,
) -> String {
    if feature.problem_flags.iter().any(|flag| flag == "tiny_bbox") {
        return "tiny_bbox_noise".to_string();
    }
    if feature.outlier_geometry || feature.problem_flags.iter().any(|flag| flag == "far_away") {
        return "source_offset_missing".to_string();
    }
    if center_delta[0].abs() < 0.25 && center_delta[1].abs() < 0.25 && center_delta[2].abs() > 0.5 {
        return "z_offset".to_string();
    }
    if is_scale_mismatch(size_ratio_xyz, diagonal_ratio) {
        return "scale_mismatch".to_string();
    }
    if looks_like_axis_swap(geometry_size, pick_size) {
        return "axis_swap".to_string();
    }
    if looks_like_sign_flip(center_delta, center_distance) {
        return "sign_flip".to_string();
    }
    if center_distance > DIAGNOSTIC_CENTER_TOLERANCE_M && diagonal_ratio < 2.0 {
        return "local_world_offset".to_string();
    }
    "unknown".to_string()
}

fn safe_ratio(a: f64, b: f64) -> f64 {
    let left = a.abs().max(0.001);
    let right = b.abs().max(0.001);
    left / right
}

fn is_scale_mismatch(size_ratio_xyz: [f64; 3], diagonal_ratio: f64) -> bool {
    let avg = (size_ratio_xyz[0] + size_ratio_xyz[1] + size_ratio_xyz[2]) / 3.0;
    let spread = size_ratio_xyz
        .iter()
        .map(|ratio| (ratio - avg).abs())
        .fold(0.0_f64, f64::max);
    (diagonal_ratio > 1.5 || diagonal_ratio < 0.67) && spread < avg.abs().max(1.0) * 0.35
}

fn looks_like_axis_swap(geometry_size: [f64; 3], pick_size: [f64; 3]) -> bool {
    let direct_error = axis_size_error(geometry_size, pick_size);
    let swaps = [
        [pick_size[1], pick_size[0], pick_size[2]],
        [pick_size[2], pick_size[1], pick_size[0]],
        [pick_size[0], pick_size[2], pick_size[1]],
    ];
    swaps
        .iter()
        .any(|candidate| axis_size_error(geometry_size, *candidate) < direct_error * 0.5)
}

fn axis_size_error(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3)
        .map(|axis| (a[axis].abs().max(0.001) - b[axis].abs().max(0.001)).abs())
        .sum()
}

fn looks_like_sign_flip(center_delta: [f64; 3], center_distance: f64) -> bool {
    center_distance > 1.0
        && center_delta
            .iter()
            .filter(|delta| delta.abs() > DIAGNOSTIC_CENTER_TOLERANCE_M)
            .count()
            == 1
}

fn nearest_center(point: [f64; 3], centers: &[[f64; 3]]) -> Option<[f64; 3]> {
    centers.iter().copied().min_by(|a, b| {
        distance3(point, *a)
            .partial_cmp(&distance3(point, *b))
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn nearest_center_distance(point: [f64; 3], centers: &[[f64; 3]]) -> Option<f64> {
    nearest_center(point, centers).map(|center| distance3(point, center))
}

fn bbox_inside_with_margin(container: [f64; 6], bbox: [f64; 6], margin: f64) -> bool {
    bbox[0] >= container[0] - margin
        && bbox[1] >= container[1] - margin
        && bbox[2] >= container[2] - margin
        && bbox[3] <= container[3] + margin
        && bbox[4] <= container[4] + margin
        && bbox[5] <= container[5] + margin
}

fn bbox_to_wgs84(source_epsg: u32, bbox: [f64; 6]) -> Option<[f64; 6]> {
    let p0 = project_to_wgs84(source_epsg, bbox[0], bbox[1]).ok()?;
    let p1 = project_to_wgs84(source_epsg, bbox[3], bbox[4]).ok()?;
    Some([
        p0.lon_deg.min(p1.lon_deg),
        p0.lat_deg.min(p1.lat_deg),
        bbox[2],
        p0.lon_deg.max(p1.lon_deg),
        p0.lat_deg.max(p1.lat_deg),
        bbox[5],
    ])
}

fn point_to_wgs84(source_epsg: u32, point: [f64; 3]) -> Option<[f64; 3]> {
    let p = project_to_wgs84(source_epsg, point[0], point[1]).ok()?;
    Some([p.lon_deg, p.lat_deg, point[2]])
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
