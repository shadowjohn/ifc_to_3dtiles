use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::{
    cad_entity_inspect::CadEntityStats,
    inspect_review::{
        InspectReviewSource, build_review_report_from_db, duplicate_review_score,
        render_review_html_with_extra,
    },
    project::ProjectManifest,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DuplicatePairCompare {
    pub source_a_id: String,
    pub source_a_name: String,
    pub source_b_id: String,
    pub source_b_name: String,
    pub score: f64,
    pub retain_source_id: String,
    pub retain_source_name: String,
    pub reject_source_id: String,
    pub reject_source_name: String,
    pub recommendation_reason: String,
    pub raw_bbox_a: [f64; 6],
    pub raw_bbox_b: [f64; 6],
    pub percentile_bbox_a: [f64; 6],
    pub percentile_bbox_b: [f64; 6],
    pub entity_count_a: u64,
    pub entity_count_b: u64,
    pub vertex_count_a: u64,
    pub vertex_count_b: u64,
    pub fingerprint_a: String,
    pub fingerprint_b: String,
    pub layer_count_diff: BTreeMap<String, CountDiff>,
    pub geometry_type_count_diff: BTreeMap<String, CountDiff>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CountDiff {
    pub left: u64,
    pub right: u64,
    pub diff: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityBboxRecord {
    pub source_id: String,
    pub fid: i64,
    pub layer: String,
    pub entity_handle: Option<String>,
    pub geometry_type: Option<String>,
    pub vertex_count: u64,
    pub bbox: [f64; 6],
}

impl EntityBboxRecord {
    pub fn new(
        source_id: impl Into<String>,
        fid: i64,
        layer: impl Into<String>,
        entity_handle: Option<String>,
        geometry_type: Option<String>,
        vertex_count: u64,
        bbox: [f64; 6],
    ) -> Self {
        Self {
            source_id: source_id.into(),
            fid,
            layer: layer.into(),
            entity_handle,
            geometry_type,
            vertex_count,
            bbox,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityOutlierReport {
    pub source_id: String,
    pub original_file_name: String,
    pub entity_count: usize,
    pub source_center: [f64; 3],
    pub outliers: Vec<EntityOutlier>,
    pub layer_outliers: Vec<LayerOutlierSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityOutlier {
    pub source_id: String,
    pub fid: i64,
    pub layer: String,
    pub entity_handle: Option<String>,
    pub geometry_type: Option<String>,
    pub vertex_count: u64,
    pub bbox: [f64; 6],
    pub reason: String,
    pub score: f64,
    pub bbox_diagonal: f64,
    pub z_range: f64,
    pub distance_from_source_center: f64,
    pub distance_from_aoi: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerOutlierSummary {
    pub layer: String,
    pub entity_count: u64,
    pub max_bbox_diagonal: f64,
    pub max_z_range: f64,
    pub max_distance_from_source_center: f64,
    pub max_distance_from_aoi: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalManifests {
    pub approved: ApprovalManifest,
    pub rejected: ApprovalManifest,
    pub needs_review: ApprovalManifest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalManifest {
    pub generated_at: String,
    pub decision: String,
    pub sources: Vec<ApprovalSourceDecision>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalSourceDecision {
    pub source_id: String,
    pub original_file_name: String,
    pub format: String,
    pub inspect_status: String,
    pub decision: String,
    pub reason: String,
    pub duplicate_of: Option<String>,
}

pub fn compare_duplicate_pair(
    source_a_name: &str,
    source_a: &CadEntityStats,
    source_b_name: &str,
    source_b: &CadEntityStats,
) -> DuplicatePairCompare {
    let score = duplicate_review_score(source_a, source_b);
    let a_preferred = source_a.entity_count >= source_b.entity_count;
    let (retain_id, retain_name, reject_id, reject_name) = if a_preferred {
        (
            source_a.source_id.clone(),
            source_a_name.to_string(),
            source_b.source_id.clone(),
            source_b_name.to_string(),
        )
    } else {
        (
            source_b.source_id.clone(),
            source_b_name.to_string(),
            source_a.source_id.clone(),
            source_a_name.to_string(),
        )
    };
    let recommendation_reason =
        "兩者 bbox/layer/entity 高度重疊；保留 entity count 較高的 source，另一筆先列 rejected duplicate candidate"
            .to_string();

    DuplicatePairCompare {
        source_a_id: source_a.source_id.clone(),
        source_a_name: source_a_name.to_string(),
        source_b_id: source_b.source_id.clone(),
        source_b_name: source_b_name.to_string(),
        score,
        retain_source_id: retain_id,
        retain_source_name: retain_name,
        reject_source_id: reject_id,
        reject_source_name: reject_name,
        recommendation_reason,
        raw_bbox_a: source_a.raw_bbox,
        raw_bbox_b: source_b.raw_bbox,
        percentile_bbox_a: source_a.percentile_bbox,
        percentile_bbox_b: source_b.percentile_bbox,
        entity_count_a: source_a.entity_count,
        entity_count_b: source_b.entity_count,
        vertex_count_a: source_a.vertex_count,
        vertex_count_b: source_b.vertex_count,
        fingerprint_a: source_a.fingerprint_hash.clone(),
        fingerprint_b: source_b.fingerprint_hash.clone(),
        layer_count_diff: count_diff(&source_a.layer_histogram, &source_b.layer_histogram),
        geometry_type_count_diff: count_diff(
            &source_a.geometry_type_histogram,
            &source_b.geometry_type_histogram,
        ),
    }
}

pub fn detect_entity_outliers(
    source_id: &str,
    original_file_name: &str,
    records: &[EntityBboxRecord],
    limit: usize,
) -> EntityOutlierReport {
    let source_center = median_center(records);
    let mut outliers = Vec::new();
    push_top_outliers(
        &mut outliers,
        records,
        source_center,
        "far_from_source_center",
        limit,
        |record, center| distance(center, bbox_center(record.bbox)),
    );
    push_top_outliers(
        &mut outliers,
        records,
        source_center,
        "largest_bbox_diagonal",
        limit,
        |record, _| bbox_diagonal(record.bbox),
    );
    push_top_outliers(
        &mut outliers,
        records,
        source_center,
        "largest_z_range",
        limit,
        |record, _| z_range(record.bbox),
    );
    push_top_outliers(
        &mut outliers,
        records,
        source_center,
        "outside_epsg3826_aoi",
        limit,
        |record, _| distance_from_aoi(record.bbox),
    );
    outliers.sort_by(|a, b| {
        a.reason
            .cmp(&b.reason)
            .then_with(|| b.score.total_cmp(&a.score))
    });

    EntityOutlierReport {
        source_id: source_id.to_string(),
        original_file_name: original_file_name.to_string(),
        entity_count: records.len(),
        source_center,
        outliers,
        layer_outliers: layer_outlier_summary(records, source_center),
    }
}

pub fn classify_approval_manifests(
    sources: &[InspectReviewSource],
    duplicate_pairs: &[DuplicatePairCompare],
) -> ApprovalManifests {
    let now = chrono_like_now();
    let mut rejected_duplicates = HashMap::new();
    for pair in duplicate_pairs {
        rejected_duplicates.insert(pair.reject_source_id.clone(), pair.retain_source_id.clone());
    }

    let mut approved = Vec::new();
    let mut rejected = Vec::new();
    let mut needs_review = Vec::new();
    for source in sources {
        if let Some(retain_source_id) = rejected_duplicates.get(&source.source_id) {
            rejected.push(decision_from_source(
                source,
                "rejected",
                "duplicate_candidate",
                Some(retain_source_id.clone()),
            ));
        } else if source.inspect_status == "approved" && source.selected_scale.is_some() {
            approved.push(decision_from_source(
                source,
                "approved",
                "entity inspect approved with selected scale",
                None,
            ));
        } else if source.inspect_status == "needs_alternative_route" {
            needs_review.push(decision_from_source(
                source,
                "needs_review",
                "DGN needs alternative route: ODA invalid group code",
                None,
            ));
        } else {
            needs_review.push(decision_from_source(
                source,
                "needs_review",
                "requires human QA before publish",
                None,
            ));
        }
    }

    ApprovalManifests {
        approved: ApprovalManifest {
            generated_at: now.clone(),
            decision: "approved".to_string(),
            sources: approved,
        },
        rejected: ApprovalManifest {
            generated_at: now.clone(),
            decision: "rejected".to_string(),
            sources: rejected,
        },
        needs_review: ApprovalManifest {
            generated_at: now,
            decision: "needs_review".to_string(),
            sources: needs_review,
        },
    }
}

pub fn write_drilldown_outputs(
    db_path: &Path,
    manifest_path: &Path,
    output_dir: &Path,
    review_report_path: &Path,
) -> Result<()> {
    fs::create_dir_all(output_dir)
        .with_context(|| format!("建立 QA 輸出目錄失敗：{}", output_dir.display()))?;
    let manifest: ProjectManifest = serde_json::from_slice(
        &fs::read(manifest_path)
            .with_context(|| format!("讀取 source manifest 失敗：{}", manifest_path.display()))?,
    )
    .with_context(|| format!("解析 source manifest 失敗：{}", manifest_path.display()))?;
    let conn = Connection::open(db_path)
        .with_context(|| format!("開啟 inspect SQLite 失敗：{}", db_path.display()))?;
    let stats = read_source_stats(&conn)?;
    let names: HashMap<String, String> = manifest
        .sources
        .iter()
        .map(|source| (source.id.clone(), source.original_file_name.clone()))
        .collect();

    let duplicate_pairs = build_duplicate_pairs(&stats, &names);
    let duplicate_path = output_dir.join("duplicate_pairs.json");
    fs::write(
        &duplicate_path,
        serde_json::to_vec_pretty(&duplicate_pairs)?,
    )
    .with_context(|| format!("寫入 duplicate_pairs 失敗：{}", duplicate_path.display()))?;

    let outlier_report =
        if let Some(center_source) = select_outlier_drilldown_source(&manifest, &stats) {
            let records = read_entity_bbox_records(&conn, &center_source.id)?;
            Some(detect_entity_outliers(
                &center_source.id,
                &center_source.original_file_name,
                &records,
                10,
            ))
        } else {
            None
        };
    let outlier_path = output_dir.join("entity_outliers.json");
    fs::write(&outlier_path, serde_json::to_vec_pretty(&outlier_report)?)
        .with_context(|| format!("寫入 entity_outliers 失敗：{}", outlier_path.display()))?;

    let review = build_review_report_from_db(db_path, manifest_path)?;
    let manifests = classify_approval_manifests(&review.sources, &duplicate_pairs);
    write_json(
        output_dir.join("approved_sources.json"),
        &manifests.approved,
    )?;
    write_json(
        output_dir.join("rejected_sources.json"),
        &manifests.rejected,
    )?;
    write_json(
        output_dir.join("needs_review_sources.json"),
        &manifests.needs_review,
    )?;

    let phase1e_html =
        render_phase1e_html_section(&duplicate_pairs, outlier_report.as_ref(), &manifests);
    fs::write(
        review_report_path,
        render_review_html_with_extra(&review, &phase1e_html),
    )
    .with_context(|| {
        format!(
            "寫入 Phase 1E review report 失敗：{}",
            review_report_path.display()
        )
    })?;
    Ok(())
}

fn select_outlier_drilldown_source<'a>(
    manifest: &'a ProjectManifest,
    stats: &HashMap<String, CadEntityStats>,
) -> Option<&'a crate::project::SourceRecord> {
    manifest
        .sources
        .iter()
        .filter(|source| stats.contains_key(&source.id))
        .max_by(|left, right| {
            let left_score = stats
                .get(&left.id)
                .map(outlier_drilldown_score)
                .unwrap_or_default();
            let right_score = stats
                .get(&right.id)
                .map(outlier_drilldown_score)
                .unwrap_or_default();
            left_score
                .partial_cmp(&right_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn outlier_drilldown_score(stats: &CadEntityStats) -> f64 {
    let bbox = stats.raw_bbox;
    let dx = (bbox[3] - bbox[0]).abs();
    let dy = (bbox[4] - bbox[1]).abs();
    let dz = (bbox[5] - bbox[2]).abs();
    (dx * dx + dy * dy + dz * dz).sqrt()
}

pub fn render_phase1e_html_section(
    duplicate_pairs: &[DuplicatePairCompare],
    outlier_report: Option<&EntityOutlierReport>,
    manifests: &ApprovalManifests,
) -> String {
    let mut html = String::new();
    html.push_str("<h2>Phase 1E QA</h2>");
    html.push_str("<div class=\"grid\">");
    html.push_str("<div class=\"box\"><b>Approval manifests</b><ul>");
    html.push_str(&format!(
        "<li>approved_sources.json：{} source(s)</li>",
        manifests.approved.sources.len()
    ));
    html.push_str(&format!(
        "<li>rejected_sources.json：{} source(s)</li>",
        manifests.rejected.sources.len()
    ));
    html.push_str(&format!(
        "<li>needs_review_sources.json：{} source(s)</li>",
        manifests.needs_review.sources.len()
    ));
    html.push_str("</ul></div>");

    html.push_str("<div class=\"box\"><b>Duplicate compare</b><ul>");
    if duplicate_pairs.is_empty() {
        html.push_str("<li class=\"muted\">none</li>");
    } else {
        for pair in duplicate_pairs {
            html.push_str(&format!(
                "<li>{} vs {} <span class=\"score\">{:.1}%</span><br><span class=\"muted\">retain {} / reject {}</span></li>",
                escape_html(&pair.source_a_name),
                escape_html(&pair.source_b_name),
                pair.score * 100.0,
                escape_html(&pair.retain_source_name),
                escape_html(&pair.reject_source_name)
            ));
        }
    }
    html.push_str("</ul></div>");

    html.push_str("<div class=\"box\"><b>Entity outliers</b><ul>");
    if let Some(report) = outlier_report {
        html.push_str(&format!(
            "<li><b>{}</b>：{} entity(s)</li>",
            escape_html(&report.original_file_name),
            report.entity_count
        ));
        for outlier in report.outliers.iter().take(8) {
            html.push_str(&format!(
                "<li>FID {} / {} / {} / score {:.3}</li>",
                outlier.fid,
                escape_html(&outlier.layer),
                escape_html(&outlier.reason),
                outlier.score
            ));
        }
    } else {
        html.push_str("<li class=\"muted\">none</li>");
    }
    html.push_str("</ul></div></div>");
    html
}

fn build_duplicate_pairs(
    stats: &HashMap<String, CadEntityStats>,
    names: &HashMap<String, String>,
) -> Vec<DuplicatePairCompare> {
    let mut items: Vec<_> = stats.iter().collect();
    items.sort_by(|a, b| a.0.cmp(b.0));
    let mut pairs = Vec::new();
    for left_idx in 0..items.len() {
        for right_idx in (left_idx + 1)..items.len() {
            let (left_id, left_stats) = items[left_idx];
            let (right_id, right_stats) = items[right_idx];
            let score = duplicate_review_score(left_stats, right_stats);
            if score >= 0.8 {
                pairs.push(compare_duplicate_pair(
                    names.get(left_id).map(String::as_str).unwrap_or(left_id),
                    left_stats,
                    names.get(right_id).map(String::as_str).unwrap_or(right_id),
                    right_stats,
                ));
            }
        }
    }
    pairs.sort_by(|a, b| b.score.total_cmp(&a.score));
    pairs
}

fn read_source_stats(conn: &Connection) -> Result<HashMap<String, CadEntityStats>> {
    let mut stmt = conn.prepare(
        "SELECT s.source_id, s.entity_count, s.parsed_entity_count, s.skipped_entity_count,
                s.vertex_count, s.raw_bbox_json, s.percentile_bbox_json, s.z_range,
                s.selected_scale, s.layer_histogram_json, s.geometry_type_histogram_json,
                COALESCE(src.inspect_status, 'pending_inspect') AS inspect_status,
                COALESCE(src.fingerprint_hash, fp.fingerprint_hash, '') AS fingerprint_hash
         FROM source_stats s
         LEFT JOIN sources src ON src.source_id = s.source_id
         LEFT JOIN fingerprints fp ON fp.source_id = s.source_id",
    )?;
    let rows = stmt.query_map([], |row| {
        let source_id: String = row.get(0)?;
        let raw_bbox_json: String = row.get(5)?;
        let percentile_bbox_json: String = row.get(6)?;
        let layer_histogram_json: String = row.get(9)?;
        let geometry_type_histogram_json: String = row.get(10)?;
        Ok((
            source_id.clone(),
            CadEntityStats {
                source_id,
                entity_count: row.get::<_, i64>(1)? as u64,
                parsed_entity_count: row.get::<_, i64>(2)? as u64,
                skipped_entity_count: row.get::<_, i64>(3)? as u64,
                vertex_count: row.get::<_, i64>(4)? as u64,
                raw_bbox: parse_bbox_json(&raw_bbox_json),
                percentile_bbox: parse_bbox_json(&percentile_bbox_json),
                z_range: row.get(7)?,
                selected_scale: row.get(8)?,
                inspect_status: row.get(11)?,
                layer_histogram: serde_json::from_str(&layer_histogram_json).unwrap_or_default(),
                geometry_type_histogram: serde_json::from_str(&geometry_type_histogram_json)
                    .unwrap_or_default(),
                fingerprint_hash: row.get(12)?,
                warnings: vec![],
            },
        ))
    })?;

    let mut stats = HashMap::new();
    for row in rows {
        let (source_id, source_stats) = row?;
        stats.insert(source_id, source_stats);
    }
    Ok(stats)
}

fn read_entity_bbox_records(conn: &Connection, source_id: &str) -> Result<Vec<EntityBboxRecord>> {
    let mut stmt = conn.prepare(
        "SELECT e.source_id, e.fid, e.layer, e.entity_handle, e.geometry_type, e.vertex_count,
                b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z
         FROM entities e
         JOIN entity_bboxes b ON b.source_id = e.source_id AND b.fid = e.fid
         WHERE e.source_id = ?1
         ORDER BY e.fid",
    )?;
    let rows = stmt.query_map([source_id], |row| {
        Ok(EntityBboxRecord {
            source_id: row.get(0)?,
            fid: row.get(1)?,
            layer: row.get(2)?,
            entity_handle: row.get(3)?,
            geometry_type: row.get(4)?,
            vertex_count: row.get::<_, i64>(5)? as u64,
            bbox: [
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                row.get(11)?,
            ],
        })
    })?;
    let mut records = Vec::new();
    for row in rows {
        records.push(row?);
    }
    Ok(records)
}

fn write_json(path: impl AsRef<Path>, value: &impl Serialize) -> Result<()> {
    let path = path.as_ref();
    fs::write(path, serde_json::to_vec_pretty(value)?)
        .with_context(|| format!("寫入 JSON 失敗：{}", path.display()))?;
    Ok(())
}

fn decision_from_source(
    source: &InspectReviewSource,
    decision: &str,
    reason: &str,
    duplicate_of: Option<String>,
) -> ApprovalSourceDecision {
    ApprovalSourceDecision {
        source_id: source.source_id.clone(),
        original_file_name: source.original_file_name.clone(),
        format: source.format.clone(),
        inspect_status: source.inspect_status.clone(),
        decision: decision.to_string(),
        reason: reason.to_string(),
        duplicate_of,
    }
}

fn count_diff(
    left: &BTreeMap<String, u64>,
    right: &BTreeMap<String, u64>,
) -> BTreeMap<String, CountDiff> {
    let mut keys: HashSet<String> = left.keys().chain(right.keys()).cloned().collect();
    let mut output = BTreeMap::new();
    for key in keys.drain() {
        let left_value = *left.get(&key).unwrap_or(&0);
        let right_value = *right.get(&key).unwrap_or(&0);
        output.insert(
            key,
            CountDiff {
                left: left_value,
                right: right_value,
                diff: left_value as i64 - right_value as i64,
            },
        );
    }
    output
}

fn push_top_outliers(
    outliers: &mut Vec<EntityOutlier>,
    records: &[EntityBboxRecord],
    source_center: [f64; 3],
    reason: &str,
    limit: usize,
    score: impl Fn(&EntityBboxRecord, [f64; 3]) -> f64,
) {
    let mut scored: Vec<_> = records
        .iter()
        .map(|record| (score(record, source_center), record))
        .filter(|(score, _)| *score > 0.0)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (score, record) in scored.into_iter().take(limit) {
        outliers.push(entity_outlier(record, source_center, reason, score));
    }
}

fn entity_outlier(
    record: &EntityBboxRecord,
    source_center: [f64; 3],
    reason: &str,
    score: f64,
) -> EntityOutlier {
    EntityOutlier {
        source_id: record.source_id.clone(),
        fid: record.fid,
        layer: record.layer.clone(),
        entity_handle: record.entity_handle.clone(),
        geometry_type: record.geometry_type.clone(),
        vertex_count: record.vertex_count,
        bbox: record.bbox,
        reason: reason.to_string(),
        score,
        bbox_diagonal: bbox_diagonal(record.bbox),
        z_range: z_range(record.bbox),
        distance_from_source_center: distance(source_center, bbox_center(record.bbox)),
        distance_from_aoi: distance_from_aoi(record.bbox),
    }
}

fn layer_outlier_summary(
    records: &[EntityBboxRecord],
    source_center: [f64; 3],
) -> Vec<LayerOutlierSummary> {
    let mut layers: BTreeMap<String, LayerOutlierSummary> = BTreeMap::new();
    for record in records {
        let entry = layers
            .entry(record.layer.clone())
            .or_insert_with(|| LayerOutlierSummary {
                layer: record.layer.clone(),
                entity_count: 0,
                max_bbox_diagonal: 0.0,
                max_z_range: 0.0,
                max_distance_from_source_center: 0.0,
                max_distance_from_aoi: 0.0,
            });
        entry.entity_count += 1;
        entry.max_bbox_diagonal = entry.max_bbox_diagonal.max(bbox_diagonal(record.bbox));
        entry.max_z_range = entry.max_z_range.max(z_range(record.bbox));
        entry.max_distance_from_source_center = entry
            .max_distance_from_source_center
            .max(distance(source_center, bbox_center(record.bbox)));
        entry.max_distance_from_aoi = entry
            .max_distance_from_aoi
            .max(distance_from_aoi(record.bbox));
    }
    let mut values: Vec<_> = layers.into_values().collect();
    values.sort_by(|a, b| {
        b.max_distance_from_source_center
            .total_cmp(&a.max_distance_from_source_center)
            .then_with(|| b.max_bbox_diagonal.total_cmp(&a.max_bbox_diagonal))
    });
    values.truncate(20);
    values
}

fn median_center(records: &[EntityBboxRecord]) -> [f64; 3] {
    if records.is_empty() {
        return [0.0; 3];
    }
    let mut xs = Vec::with_capacity(records.len());
    let mut ys = Vec::with_capacity(records.len());
    let mut zs = Vec::with_capacity(records.len());
    for record in records {
        let center = bbox_center(record.bbox);
        xs.push(center[0]);
        ys.push(center[1]);
        zs.push(center[2]);
    }
    xs.sort_by(f64::total_cmp);
    ys.sort_by(f64::total_cmp);
    zs.sort_by(f64::total_cmp);
    [median(&xs), median(&ys), median(&zs)]
}

fn median(values: &[f64]) -> f64 {
    values[values.len() / 2]
}

fn bbox_center(bbox: [f64; 6]) -> [f64; 3] {
    [
        (bbox[0] + bbox[3]) * 0.5,
        (bbox[1] + bbox[4]) * 0.5,
        (bbox[2] + bbox[5]) * 0.5,
    ]
}

fn bbox_diagonal(bbox: [f64; 6]) -> f64 {
    let dx = bbox[3] - bbox[0];
    let dy = bbox[4] - bbox[1];
    let dz = bbox[5] - bbox[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn z_range(bbox: [f64; 6]) -> f64 {
    (bbox[5] - bbox[2]).abs()
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn distance_from_aoi(bbox: [f64; 6]) -> f64 {
    let center = bbox_center(bbox);
    let dx = if center[0] < 120_000.0 {
        120_000.0 - center[0]
    } else if center[0] > 360_000.0 {
        center[0] - 360_000.0
    } else {
        0.0
    };
    let dy = if center[1] < 2_400_000.0 {
        2_400_000.0 - center[1]
    } else if center[1] > 2_800_000.0 {
        center[1] - 2_800_000.0
    } else {
        0.0
    };
    (dx * dx + dy * dy).sqrt()
}

fn parse_bbox_json(text: &str) -> [f64; 6] {
    serde_json::from_str(text).unwrap_or([0.0; 6])
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn chrono_like_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| format!("unix:{}", duration.as_secs()))
        .unwrap_or_else(|_| "unix:0".to_string())
}
