use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    crs::project_to_wgs84,
    inspect_drilldown::{ApprovalManifests, DuplicatePairCompare, EntityOutlierReport},
    inspect_review::{InspectReviewReport, InspectReviewSource, build_review_report_from_db},
    project::ProjectManifest,
};

const DEFAULT_AOI_EPSG3826: [f64; 4] = [120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialQaManifest {
    pub generated_at: String,
    pub project_id: String,
    pub source_epsg: u32,
    pub publish_runtime_source_ids: Vec<String>,
    pub debug_overlay_source_ids: Vec<String>,
    pub aoi: SpatialQaAoi,
    pub sources: Vec<SpatialQaSource>,
    pub duplicate_pairs: Vec<SpatialQaDuplicatePair>,
    pub outliers: Vec<SpatialQaOutlier>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialQaAoi {
    pub epsg: u32,
    pub epsg3826_bbox: [f64; 4],
    pub wgs84_bbox: [f64; 4],
    pub wgs84_polygon: Vec<[f64; 2]>,
}

impl SpatialQaAoi {
    pub fn epsg3826_default() -> Result<Self> {
        let b = DEFAULT_AOI_EPSG3826;
        let corners = [
            [b[0], b[1]],
            [b[2], b[1]],
            [b[2], b[3]],
            [b[0], b[3]],
            [b[0], b[1]],
        ];
        let mut polygon = Vec::new();
        for corner in corners {
            let lonlat = project_to_wgs84(3826, corner[0], corner[1])?;
            polygon.push([lonlat.lon_deg, lonlat.lat_deg]);
        }
        let min_lon = polygon
            .iter()
            .map(|point| point[0])
            .fold(f64::INFINITY, f64::min);
        let min_lat = polygon
            .iter()
            .map(|point| point[1])
            .fold(f64::INFINITY, f64::min);
        let max_lon = polygon
            .iter()
            .map(|point| point[0])
            .fold(f64::NEG_INFINITY, f64::max);
        let max_lat = polygon
            .iter()
            .map(|point| point[1])
            .fold(f64::NEG_INFINITY, f64::max);
        Ok(Self {
            epsg: 3826,
            epsg3826_bbox: b,
            wgs84_bbox: [min_lon, min_lat, max_lon, max_lat],
            wgs84_polygon: polygon,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialQaSource {
    pub source_id: String,
    pub original_file_name: String,
    pub format: String,
    pub inspect_status: String,
    pub approval_decision: String,
    pub approval_reason: String,
    pub duplicate_of: Option<String>,
    pub selected_scale: Option<f64>,
    pub entity_count: u64,
    pub vertex_count: u64,
    pub raw_bbox: Option<[f64; 6]>,
    pub raw_bbox_wgs84: Option<[f64; 6]>,
    pub percentile_bbox: Option<[f64; 6]>,
    pub percentile_bbox_wgs84: Option<[f64; 6]>,
    pub aoi_status: String,
    pub aoi_gap_m: Option<[f64; 4]>,
    pub bbox_inflation_ratio: Option<f64>,
    pub warnings: Vec<String>,
    pub quarantine_reasons: Vec<String>,
    pub duplicate_candidates: Vec<SpatialQaDuplicateCandidate>,
    pub top_layers: Vec<SpatialQaCount>,
    pub geometry_types: Vec<SpatialQaCount>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialQaDuplicateCandidate {
    pub source_id: String,
    pub original_file_name: String,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialQaCount {
    pub name: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialQaDuplicatePair {
    pub source_a_id: String,
    pub source_a_name: String,
    pub source_b_id: String,
    pub source_b_name: String,
    pub score: f64,
    pub retain_source_id: String,
    pub reject_source_id: String,
    pub recommendation_reason: String,
    pub source_a_percentile_bbox_wgs84: Option<[f64; 6]>,
    pub source_b_percentile_bbox_wgs84: Option<[f64; 6]>,
    pub source_a_raw_bbox_wgs84: Option<[f64; 6]>,
    pub source_b_raw_bbox_wgs84: Option<[f64; 6]>,
    pub entity_count_a: u64,
    pub entity_count_b: u64,
    pub vertex_count_a: u64,
    pub vertex_count_b: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialQaOutlier {
    pub source_id: String,
    pub original_file_name: String,
    pub fid: i64,
    pub layer: String,
    pub entity_handle: Option<String>,
    pub geometry_type: Option<String>,
    pub vertex_count: u64,
    pub bbox: [f64; 6],
    pub bbox_wgs84: Option<[f64; 6]>,
    pub center_wgs84: Option<[f64; 3]>,
    pub reason: String,
    pub score: f64,
    pub distance_from_aoi: f64,
    pub distance_from_source_center: f64,
}

pub fn build_spatial_qa_manifest(
    manifest: &ProjectManifest,
    review: &InspectReviewReport,
    approvals: &ApprovalManifests,
    duplicate_pairs: &[DuplicatePairCompare],
    outlier_reports: &[EntityOutlierReport],
) -> SpatialQaManifest {
    let approval_map = approval_map(approvals);
    let publish_runtime_source_ids = approvals
        .approved
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<Vec<_>>();
    let debug_overlay_source_ids = approvals
        .rejected
        .sources
        .iter()
        .chain(approvals.needs_review.sources.iter())
        .map(|source| source.source_id.clone())
        .collect::<Vec<_>>();
    let mut sources = Vec::new();
    for source in &review.sources {
        let (approval_decision, approval_reason, duplicate_of) = approval_map
            .get(&source.source_id)
            .cloned()
            .unwrap_or_else(|| {
                (
                    source.inspect_status.clone(),
                    "not present in approval manifest".to_string(),
                    None,
                )
            });
        sources.push(spatial_source_from_review(
            source,
            approval_decision,
            approval_reason,
            duplicate_of,
        ));
    }

    SpatialQaManifest {
        generated_at: chrono_like_now(),
        project_id: manifest.project_id.clone(),
        source_epsg: manifest.source_epsg,
        publish_runtime_source_ids,
        debug_overlay_source_ids,
        aoi: SpatialQaAoi::epsg3826_default().unwrap_or_else(|_| SpatialQaAoi {
            epsg: 3826,
            epsg3826_bbox: DEFAULT_AOI_EPSG3826,
            wgs84_bbox: [0.0, 0.0, 0.0, 0.0],
            wgs84_polygon: vec![],
        }),
        sources,
        duplicate_pairs: duplicate_pairs.iter().map(spatial_duplicate_pair).collect(),
        outliers: outlier_reports.iter().flat_map(spatial_outliers).collect(),
    }
}

pub fn write_spatial_qa_manifest(input: &Path, output: &Path) -> Result<SpatialQaManifest> {
    let manifest_path = input.join("source_manifest.json");
    let db_path = input.join("project_inspect.db");
    let qa_dir = input.join("qa");
    let manifest: ProjectManifest = read_json(&manifest_path)?;
    let review = build_review_report_from_db(&db_path, &manifest_path)?;
    let approvals = read_approval_manifests(&qa_dir)?;
    let duplicate_pairs: Vec<DuplicatePairCompare> =
        read_json(&qa_dir.join("duplicate_pairs.json"))
            .with_context(|| "讀取 Phase 1E duplicate_pairs.json 失敗".to_string())?;
    let outlier_report: Option<EntityOutlierReport> =
        read_json(&qa_dir.join("entity_outliers.json"))
            .with_context(|| "讀取 Phase 1E entity_outliers.json 失敗".to_string())?;
    let outlier_reports = outlier_report.into_iter().collect::<Vec<_>>();
    let spatial = build_spatial_qa_manifest(
        &manifest,
        &review,
        &approvals,
        &duplicate_pairs,
        &outlier_reports,
    );
    fs::write(
        output.join("spatial_qa_manifest.json"),
        serde_json::to_vec_pretty(&spatial)?,
    )
    .with_context(|| format!("寫入 spatial_qa_manifest 失敗：{}", output.display()))?;
    Ok(spatial)
}

pub fn render_spatial_qa_review_summary(
    manifest_path: &str,
    aoi: &SpatialQaAoi,
    source_count: usize,
    duplicate_count: usize,
    outlier_count: usize,
) -> String {
    format!(
        r#"<h2>Phase 1G Spatial QA</h2><div class="grid"><div class="box"><b>Spatial QA manifest</b><ul><li><a href="{manifest_path}">{manifest_path}</a></li><li>source detail：{source_count}</li><li>duplicate pair：{duplicate_count}</li><li>outlier marker：{outlier_count}</li></ul></div><div class="box"><b>AOI</b><ul><li>EPSG:{} bbox：{:.0}, {:.0}, {:.0}, {:.0}</li><li>Viewer：bbox click / detail panel / AOI / duplicate compare / outlier marker</li></ul></div></div>"#,
        aoi.epsg,
        aoi.epsg3826_bbox[0],
        aoi.epsg3826_bbox[1],
        aoi.epsg3826_bbox[2],
        aoi.epsg3826_bbox[3],
    )
}

fn spatial_source_from_review(
    source: &InspectReviewSource,
    approval_decision: String,
    approval_reason: String,
    duplicate_of: Option<String>,
) -> SpatialQaSource {
    let aoi_gap_m = source.percentile_bbox.map(bbox_aoi_gap_m);
    let aoi_status = aoi_status(aoi_gap_m);
    let bbox_inflation_ratio = bbox_inflation_ratio(source.raw_bbox, source.percentile_bbox);
    SpatialQaSource {
        source_id: source.source_id.clone(),
        original_file_name: source.original_file_name.clone(),
        format: source.format.clone(),
        inspect_status: source.inspect_status.clone(),
        approval_decision,
        approval_reason,
        duplicate_of,
        selected_scale: source.selected_scale,
        entity_count: source.entity_count,
        vertex_count: source.vertex_count,
        raw_bbox: source.raw_bbox,
        raw_bbox_wgs84: source.raw_bbox.and_then(bbox_to_wgs84),
        percentile_bbox: source.percentile_bbox,
        percentile_bbox_wgs84: source.percentile_bbox.and_then(bbox_to_wgs84),
        aoi_status,
        aoi_gap_m,
        bbox_inflation_ratio,
        warnings: source.warnings.clone(),
        quarantine_reasons: source.quarantine_reasons.clone(),
        duplicate_candidates: source
            .duplicate_candidates
            .iter()
            .map(|candidate| SpatialQaDuplicateCandidate {
                source_id: candidate.source_id.clone(),
                original_file_name: candidate.original_file_name.clone(),
                score: candidate.score,
            })
            .collect(),
        top_layers: top_counts(&source.layer_histogram, 8),
        geometry_types: top_counts(&source.geometry_type_histogram, 8),
    }
}

fn spatial_duplicate_pair(pair: &DuplicatePairCompare) -> SpatialQaDuplicatePair {
    SpatialQaDuplicatePair {
        source_a_id: pair.source_a_id.clone(),
        source_a_name: pair.source_a_name.clone(),
        source_b_id: pair.source_b_id.clone(),
        source_b_name: pair.source_b_name.clone(),
        score: pair.score,
        retain_source_id: pair.retain_source_id.clone(),
        reject_source_id: pair.reject_source_id.clone(),
        recommendation_reason: pair.recommendation_reason.clone(),
        source_a_percentile_bbox_wgs84: bbox_to_wgs84(pair.percentile_bbox_a),
        source_b_percentile_bbox_wgs84: bbox_to_wgs84(pair.percentile_bbox_b),
        source_a_raw_bbox_wgs84: bbox_to_wgs84(pair.raw_bbox_a),
        source_b_raw_bbox_wgs84: bbox_to_wgs84(pair.raw_bbox_b),
        entity_count_a: pair.entity_count_a,
        entity_count_b: pair.entity_count_b,
        vertex_count_a: pair.vertex_count_a,
        vertex_count_b: pair.vertex_count_b,
    }
}

fn spatial_outliers(report: &EntityOutlierReport) -> Vec<SpatialQaOutlier> {
    report
        .outliers
        .iter()
        .map(|outlier| {
            let center = bbox_center(outlier.bbox);
            SpatialQaOutlier {
                source_id: outlier.source_id.clone(),
                original_file_name: report.original_file_name.clone(),
                fid: outlier.fid,
                layer: outlier.layer.clone(),
                entity_handle: outlier.entity_handle.clone(),
                geometry_type: outlier.geometry_type.clone(),
                vertex_count: outlier.vertex_count,
                bbox: outlier.bbox,
                bbox_wgs84: bbox_to_wgs84(outlier.bbox),
                center_wgs84: point_to_wgs84(center),
                reason: outlier.reason.clone(),
                score: outlier.score,
                distance_from_aoi: outlier.distance_from_aoi,
                distance_from_source_center: outlier.distance_from_source_center,
            }
        })
        .collect()
}

fn top_counts(histogram: &BTreeMap<String, u64>, limit: usize) -> Vec<SpatialQaCount> {
    let mut counts = histogram
        .iter()
        .map(|(name, count)| SpatialQaCount {
            name: name.clone(),
            count: *count,
        })
        .collect::<Vec<_>>();
    counts.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    counts.truncate(limit);
    counts
}

fn approval_map(
    approvals: &ApprovalManifests,
) -> BTreeMap<String, (String, String, Option<String>)> {
    let mut map = BTreeMap::new();
    for decision in approvals
        .approved
        .sources
        .iter()
        .chain(approvals.rejected.sources.iter())
        .chain(approvals.needs_review.sources.iter())
    {
        map.insert(
            decision.source_id.clone(),
            (
                decision.decision.clone(),
                decision.reason.clone(),
                decision.duplicate_of.clone(),
            ),
        );
    }
    map
}

fn bbox_to_wgs84(bbox: [f64; 6]) -> Option<[f64; 6]> {
    let min = project_to_wgs84(3826, bbox[0], bbox[1]).ok()?;
    let max = project_to_wgs84(3826, bbox[3], bbox[4]).ok()?;
    Some([
        min.lon_deg.min(max.lon_deg),
        min.lat_deg.min(max.lat_deg),
        bbox[2],
        min.lon_deg.max(max.lon_deg),
        min.lat_deg.max(max.lat_deg),
        bbox[5],
    ])
}

fn point_to_wgs84(point: [f64; 3]) -> Option<[f64; 3]> {
    let lonlat = project_to_wgs84(3826, point[0], point[1]).ok()?;
    Some([lonlat.lon_deg, lonlat.lat_deg, point[2]])
}

fn bbox_center(bbox: [f64; 6]) -> [f64; 3] {
    [
        (bbox[0] + bbox[3]) * 0.5,
        (bbox[1] + bbox[4]) * 0.5,
        (bbox[2] + bbox[5]) * 0.5,
    ]
}

fn bbox_aoi_gap_m(bbox: [f64; 6]) -> [f64; 4] {
    let aoi = DEFAULT_AOI_EPSG3826;
    [
        (aoi[0] - bbox[0]).max(0.0),
        (aoi[1] - bbox[1]).max(0.0),
        (bbox[3] - aoi[2]).max(0.0),
        (bbox[4] - aoi[3]).max(0.0),
    ]
}

fn aoi_status(gap: Option<[f64; 4]>) -> String {
    match gap {
        None => "no_bbox".to_string(),
        Some(gap) if gap.iter().any(|value| *value > 0.001) => "outside_aoi".to_string(),
        Some(_) => "inside_aoi".to_string(),
    }
}

fn bbox_inflation_ratio(
    raw_bbox: Option<[f64; 6]>,
    percentile_bbox: Option<[f64; 6]>,
) -> Option<f64> {
    let raw = raw_bbox?;
    let percentile = percentile_bbox?;
    let raw_area = bbox_xy_area(raw);
    let percentile_area = bbox_xy_area(percentile);
    if percentile_area <= 0.000_001 {
        None
    } else {
        Some((raw_area / percentile_area).max(1.0))
    }
}

fn bbox_xy_area(bbox: [f64; 6]) -> f64 {
    ((bbox[3] - bbox[0]).abs() * (bbox[4] - bbox[1]).abs()).max(0.0)
}

fn read_approval_manifests(qa_dir: &Path) -> Result<ApprovalManifests> {
    Ok(ApprovalManifests {
        approved: read_json(&qa_dir.join("approved_sources.json"))?,
        rejected: read_json(&qa_dir.join("rejected_sources.json"))?,
        needs_review: read_json(&qa_dir.join("needs_review_sources.json"))?,
    })
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    serde_json::from_slice(
        &fs::read(path).with_context(|| format!("讀取 JSON 失敗：{}", path.display()))?,
    )
    .with_context(|| format!("解析 JSON 失敗：{}", path.display()))
}

fn chrono_like_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| format!("unix:{}", duration.as_secs()))
        .unwrap_or_else(|_| "unix:0".to_string())
}
