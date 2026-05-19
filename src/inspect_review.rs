use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::Path,
};

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::{
    cad_entity_inspect::CadEntityStats,
    project::{ProjectManifest, SourceFormat, SourceRecord, SourceStatus},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InspectReviewReport {
    pub project_id: String,
    pub generated_at: String,
    pub source_count: usize,
    pub sources: Vec<InspectReviewSource>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InspectReviewSource {
    pub source_id: String,
    pub display_name: String,
    pub original_file_name: String,
    pub format: String,
    pub inspect_status: String,
    pub selected_scale: Option<f64>,
    pub entity_count: u64,
    pub parsed_entity_count: u64,
    pub skipped_entity_count: u64,
    pub vertex_count: u64,
    pub raw_bbox: Option<[f64; 6]>,
    pub percentile_bbox: Option<[f64; 6]>,
    pub z_range: Option<f64>,
    pub fingerprint_hash: Option<String>,
    pub layer_histogram: BTreeMap<String, u64>,
    pub geometry_type_histogram: BTreeMap<String, u64>,
    pub warnings: Vec<String>,
    pub quarantine_reasons: Vec<String>,
    pub duplicate_candidates: Vec<InspectDuplicateCandidate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InspectDuplicateCandidate {
    pub source_id: String,
    pub original_file_name: String,
    pub score: f64,
}

impl InspectReviewSource {
    pub fn from_stats(
        source_id: &str,
        original_file_name: &str,
        format: &str,
        inspect_status: &str,
        stats: &CadEntityStats,
        warnings: Vec<String>,
    ) -> Self {
        let mut merged_warnings = stats.warnings.clone();
        for warning in warnings {
            if !merged_warnings.contains(&warning) {
                merged_warnings.push(warning);
            }
        }
        let quarantine_reasons = classify_quarantine_reasons(
            inspect_status,
            stats.selected_scale,
            Some(stats.z_range),
            &merged_warnings,
            Some(stats),
        );
        Self {
            source_id: source_id.to_string(),
            display_name: original_file_name.to_string(),
            original_file_name: original_file_name.to_string(),
            format: format.to_string(),
            inspect_status: inspect_status.to_string(),
            selected_scale: stats.selected_scale,
            entity_count: stats.entity_count,
            parsed_entity_count: stats.parsed_entity_count,
            skipped_entity_count: stats.skipped_entity_count,
            vertex_count: stats.vertex_count,
            raw_bbox: Some(stats.raw_bbox),
            percentile_bbox: Some(stats.percentile_bbox),
            z_range: Some(stats.z_range),
            fingerprint_hash: Some(stats.fingerprint_hash.clone()),
            layer_histogram: stats.layer_histogram.clone(),
            geometry_type_histogram: stats.geometry_type_histogram.clone(),
            warnings: merged_warnings,
            quarantine_reasons,
            duplicate_candidates: vec![],
        }
    }

    pub fn add_duplicate_candidate(
        &mut self,
        source_id: &str,
        original_file_name: &str,
        score: f64,
    ) {
        self.duplicate_candidates.push(InspectDuplicateCandidate {
            source_id: source_id.to_string(),
            original_file_name: original_file_name.to_string(),
            score,
        });
        self.duplicate_candidates
            .sort_by(|a, b| b.score.total_cmp(&a.score));
        if score >= 0.8
            && !self
                .quarantine_reasons
                .iter()
                .any(|reason| reason.contains("重疊候選"))
        {
            self.quarantine_reasons
                .push("有高分重疊候選，publish 前需人工確認是否 duplicate".to_string());
        }
    }

    fn from_manifest_without_stats(source: &SourceRecord) -> Self {
        let status = source
            .inspect_status
            .clone()
            .unwrap_or_else(|| source_status_text(source.status).to_string());
        let warnings = source.warnings.clone();
        let quarantine_reasons =
            classify_quarantine_reasons(&status, source.selected_scale, None, &warnings, None);
        Self {
            source_id: source.id.clone(),
            display_name: source.display_name.clone(),
            original_file_name: source.original_file_name.clone(),
            format: source_format_text(source.format).to_string(),
            inspect_status: status,
            selected_scale: source.selected_scale,
            entity_count: 0,
            parsed_entity_count: 0,
            skipped_entity_count: 0,
            vertex_count: 0,
            raw_bbox: source.raw_bbox,
            percentile_bbox: source.percentile_bbox,
            z_range: None,
            fingerprint_hash: source.fingerprint_hash.clone(),
            layer_histogram: BTreeMap::new(),
            geometry_type_histogram: BTreeMap::new(),
            warnings,
            quarantine_reasons,
            duplicate_candidates: vec![],
        }
    }
}

pub fn build_review_report_from_db(
    db_path: &Path,
    manifest_path: &Path,
) -> Result<InspectReviewReport> {
    let manifest: ProjectManifest = serde_json::from_slice(
        &fs::read(manifest_path)
            .with_context(|| format!("讀取 source manifest 失敗：{}", manifest_path.display()))?,
    )
    .with_context(|| format!("解析 source manifest 失敗：{}", manifest_path.display()))?;
    let conn = Connection::open(db_path)
        .with_context(|| format!("開啟 inspect SQLite 失敗：{}", db_path.display()))?;
    let stats = read_source_stats(&conn)?;
    let db_warnings = read_warnings(&conn)?;
    let mut report_sources = Vec::new();

    for source in &manifest.sources {
        if let Some(source_stats) = stats.get(&source.id) {
            let mut warnings = source.warnings.clone();
            if let Some(extra) = db_warnings.get(&source.id) {
                for warning in extra {
                    if !warnings.contains(warning) {
                        warnings.push(warning.clone());
                    }
                }
            }
            let status = source
                .inspect_status
                .clone()
                .unwrap_or_else(|| source_status_text(source.status).to_string());
            let mut review_source = InspectReviewSource::from_stats(
                &source.id,
                &source.original_file_name,
                source_format_text(source.format),
                &status,
                source_stats,
                warnings,
            );
            review_source.display_name = source.display_name.clone();
            report_sources.push(review_source);
        } else {
            report_sources.push(InspectReviewSource::from_manifest_without_stats(source));
        }
    }

    add_duplicate_candidates(&mut report_sources, &stats);
    Ok(InspectReviewReport {
        project_id: manifest.project_id,
        generated_at: chrono_like_now(),
        source_count: report_sources.len(),
        sources: report_sources,
    })
}

pub fn write_review_report_html(db_path: &Path, manifest_path: &Path, output: &Path) -> Result<()> {
    let report = build_review_report_from_db(db_path, manifest_path)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("建立 review report 目錄失敗：{}", parent.display()))?;
    }
    fs::write(output, render_review_html(&report))
        .with_context(|| format!("寫入 review report 失敗：{}", output.display()))?;
    Ok(())
}

pub fn duplicate_review_score(a: &CadEntityStats, b: &CadEntityStats) -> f64 {
    let bbox_score = bbox_similarity(a.percentile_bbox, b.percentile_bbox);
    let entity_score = ratio_score(a.entity_count, b.entity_count);
    let vertex_score = ratio_score(a.vertex_count, b.vertex_count);
    let layer_score = histogram_overlap(&a.layer_histogram, &b.layer_histogram);
    (bbox_score * 0.45 + entity_score * 0.20 + vertex_score * 0.20 + layer_score * 0.15)
        .clamp(0.0, 1.0)
}

pub fn render_review_html(report: &InspectReviewReport) -> String {
    render_review_html_with_extra(report, "")
}

pub fn render_review_html_with_extra(report: &InspectReviewReport, extra_html: &str) -> String {
    let mut html = String::new();
    html.push_str("<!doctype html><html lang=\"zh-Hant\"><head><meta charset=\"utf-8\">");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    html.push_str("<title>Phase 1D Inspect Review</title>");
    html.push_str("<style>");
    html.push_str(
        r#"
        :root { color-scheme: dark; --bg:#101418; --panel:#181f26; --line:#2b3642; --text:#e8eef5; --muted:#91a0ad; --ok:#2fb36d; --warn:#d69a28; --bad:#e06565; --info:#4aa3df; }
        * { box-sizing:border-box; }
        body { margin:0; background:var(--bg); color:var(--text); font-family:"Segoe UI", Arial, sans-serif; font-size:14px; }
        header { padding:20px 24px; border-bottom:1px solid var(--line); background:#0d1116; position:sticky; top:0; z-index:2; }
        h1 { margin:0 0 8px; font-size:22px; font-weight:650; letter-spacing:0; }
        h2 { margin:24px 0 12px; font-size:17px; }
        main { padding:18px 24px 32px; }
        .summary { display:grid; grid-template-columns:repeat(auto-fit,minmax(150px,1fr)); gap:10px; margin-top:12px; }
        .metric { background:var(--panel); border:1px solid var(--line); border-radius:8px; padding:12px; }
        .metric b { display:block; font-size:22px; margin-bottom:3px; }
        table { width:100%; border-collapse:collapse; background:var(--panel); border:1px solid var(--line); }
        th, td { border-bottom:1px solid var(--line); padding:8px 10px; text-align:left; vertical-align:top; }
        th { color:#d6e2ef; font-weight:600; background:#121920; position:sticky; top:82px; z-index:1; }
        tr:hover td { background:#1e2831; }
        .badge { display:inline-block; padding:2px 8px; border-radius:999px; font-size:12px; border:1px solid var(--line); white-space:nowrap; }
        .approved { color:#a9f0c9; border-color:#2f8f59; background:#10351f; }
        .quarantined { color:#ffd2d2; border-color:#9f4747; background:#351818; }
        .pending_inspect, .needs_alternative_route { color:#ffe3a3; border-color:#9b7422; background:#35290e; }
        .source-card { margin:14px 0; background:var(--panel); border:1px solid var(--line); border-radius:8px; overflow:hidden; }
        .source-card > summary { cursor:pointer; padding:12px 14px; list-style:none; display:flex; gap:12px; align-items:center; flex-wrap:wrap; }
        .source-card > summary::-webkit-details-marker { display:none; }
        .source-body { padding:0 14px 14px; }
        .grid { display:grid; grid-template-columns:repeat(auto-fit,minmax(260px,1fr)); gap:12px; }
        .box { border:1px solid var(--line); border-radius:8px; padding:10px; background:#121920; }
        .mono { font-family:Consolas, "Cascadia Mono", monospace; color:#dbe9f8; }
        .muted { color:var(--muted); }
        ul { margin:6px 0 0 18px; padding:0; }
        li { margin:4px 0; }
        .score { font-weight:650; color:#ffe08a; }
        "#,
    );
    html.push_str("</style></head><body>");
    html.push_str("<header><h1>Phase 1D Inspect Review</h1>");
    html.push_str(&format!(
        "<div class=\"muted\">Project: {} · Generated: {} · Sources: {}</div>",
        escape_html(&report.project_id),
        escape_html(&report.generated_at),
        report.source_count
    ));
    html.push_str("<div class=\"summary\">");
    for (label, count) in status_counts(report) {
        html.push_str(&format!(
            "<div class=\"metric\"><b>{count}</b><span>{}</span></div>",
            escape_html(&label)
        ));
    }
    html.push_str("</div></header><main>");
    html.push_str("<h2>Source List</h2><table><thead><tr>");
    html.push_str("<th>Source</th><th>Status</th><th>Scale</th><th>Entities</th><th>P0.5/P99.5 bbox</th><th>Warnings</th><th>Duplicate</th>");
    html.push_str("</tr></thead><tbody>");
    for source in &report.sources {
        html.push_str("<tr>");
        html.push_str(&format!(
            "<td><b>{}</b><div class=\"muted mono\">{}</div></td>",
            escape_html(&source.original_file_name),
            escape_html(&source.source_id)
        ));
        html.push_str(&format!(
            "<td><span class=\"badge {}\">{}</span></td>",
            escape_attr(&source.inspect_status),
            escape_html(&source.inspect_status)
        ));
        html.push_str(&format!(
            "<td>{}</td><td>{}</td><td class=\"mono\">{}</td><td>{}</td><td>{}</td>",
            format_scale(source.selected_scale),
            format_u64(source.entity_count),
            format_bbox(source.percentile_bbox),
            source.warnings.len(),
            format_duplicate_summary(source)
        ));
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table>");

    html.push_str("<h2>Quarantine / Source Detail</h2>");
    for source in &report.sources {
        html.push_str("<details class=\"source-card\" open><summary>");
        html.push_str(&format!(
            "<span class=\"badge {}\">{}</span><b>{}</b><span class=\"muted mono\">{}</span>",
            escape_attr(&source.inspect_status),
            escape_html(&source.inspect_status),
            escape_html(&source.original_file_name),
            escape_html(&source.source_id)
        ));
        html.push_str("</summary><div class=\"source-body\"><div class=\"grid\">");
        html.push_str("<div class=\"box\"><b>Scale / bbox</b>");
        html.push_str(&format!(
            "<p>selected_scale: <span class=\"mono\">{}</span></p>",
            format_scale(source.selected_scale)
        ));
        html.push_str(&format!(
            "<p>raw bbox: <span class=\"mono\">{}</span></p>",
            format_bbox(source.raw_bbox)
        ));
        html.push_str(&format!(
            "<p>P0.5/P99.5 bbox: <span class=\"mono\">{}</span></p>",
            format_bbox(source.percentile_bbox)
        ));
        html.push_str(&format!(
            "<p>z range: <span class=\"mono\">{}</span></p></div>",
            source
                .z_range
                .map(|value| format!("{value:.3} m"))
                .unwrap_or_else(|| "-".to_string())
        ));

        html.push_str("<div class=\"box\"><b>Quarantine reason</b>");
        push_list(&mut html, &source.quarantine_reasons);
        html.push_str("</div><div class=\"box\"><b>Warnings</b>");
        push_list(&mut html, &source.warnings);
        html.push_str("</div><div class=\"box\"><b>Duplicate candidates</b><ul>");
        if source.duplicate_candidates.is_empty() {
            html.push_str("<li class=\"muted\">none</li>");
        } else {
            for candidate in &source.duplicate_candidates {
                html.push_str(&format!(
                    "<li>{} <span class=\"muted mono\">{}</span> <span class=\"score\">{:.1}%</span></li>",
                    escape_html(&candidate.original_file_name),
                    escape_html(&candidate.source_id),
                    candidate.score * 100.0
                ));
            }
        }
        html.push_str("</ul></div>");
        html.push_str("<div class=\"box\"><b>Top layers</b>");
        push_histogram(&mut html, &source.layer_histogram, source.entity_count);
        html.push_str("</div><div class=\"box\"><b>Geometry types</b>");
        push_histogram(
            &mut html,
            &source.geometry_type_histogram,
            source.parsed_entity_count,
        );
        html.push_str("</div></div></div></details>");
    }

    html.push_str(extra_html);
    html.push_str("</main></body></html>");
    html
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
                layer_histogram: parse_json_map(&layer_histogram_json),
                geometry_type_histogram: parse_json_map(&geometry_type_histogram_json),
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

fn read_warnings(conn: &Connection) -> Result<HashMap<String, Vec<String>>> {
    let mut warnings: HashMap<String, Vec<String>> = HashMap::new();
    let mut stmt =
        conn.prepare("SELECT source_id, message FROM warnings ORDER BY source_id, id")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (source_id, message) = row?;
        warnings.entry(source_id).or_default().push(message);
    }
    Ok(warnings)
}

fn add_duplicate_candidates(
    report_sources: &mut [InspectReviewSource],
    stats: &HashMap<String, CadEntityStats>,
) {
    let mut pairs = Vec::new();
    let stat_items: Vec<_> = stats.iter().collect();
    for i in 0..stat_items.len() {
        for j in (i + 1)..stat_items.len() {
            let (left_id, left_stats) = stat_items[i];
            let (right_id, right_stats) = stat_items[j];
            let score = duplicate_review_score(left_stats, right_stats);
            if score >= 0.8 {
                pairs.push((left_id.clone(), right_id.clone(), score));
            }
        }
    }

    let names: HashMap<_, _> = report_sources
        .iter()
        .map(|source| (source.source_id.clone(), source.original_file_name.clone()))
        .collect();
    for (left_id, right_id, score) in pairs {
        let left_name = names
            .get(&left_id)
            .cloned()
            .unwrap_or_else(|| left_id.clone());
        let right_name = names
            .get(&right_id)
            .cloned()
            .unwrap_or_else(|| right_id.clone());
        if let Some(source) = report_sources
            .iter_mut()
            .find(|source| source.source_id == left_id)
        {
            source.add_duplicate_candidate(&right_id, &right_name, score);
        }
        if let Some(source) = report_sources
            .iter_mut()
            .find(|source| source.source_id == right_id)
        {
            source.add_duplicate_candidate(&left_id, &left_name, score);
        }
    }
}

fn classify_quarantine_reasons(
    inspect_status: &str,
    selected_scale: Option<f64>,
    z_range: Option<f64>,
    warnings: &[String],
    stats: Option<&CadEntityStats>,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if inspect_status == "approved" {
        return reasons;
    }
    if inspect_status == "needs_alternative_route" {
        reasons.push(
            "DGN 暫停主線：需要 alternative route，原因是 ODA invalid group code".to_string(),
        );
    }
    if inspect_status == "pending_inspect" {
        reasons.push("Phase 1C 尚未取得 entity-level inspect 統計".to_string());
    }
    if stats.is_none()
        && inspect_status != "pending_inspect"
        && inspect_status != "needs_alternative_route"
    {
        reasons.push("沒有可用的 DXF entity-level bbox / layer 統計".to_string());
    }
    if selected_scale.is_none() && stats.is_some() {
        reasons.push("Scale classifier 無法在 allowed scales 選出可信比例".to_string());
    }
    if warnings
        .iter()
        .any(|warning| warning.to_ascii_lowercase().contains("outside aoi"))
    {
        reasons.push("超出 AOI：P0.5/P99.5 bbox 在所有 allowed scales 下仍不可信".to_string());
    }
    if let Some(z_range) = z_range {
        if z_range < 0.05 {
            reasons.push(format!("可能是 2D：P0.5/P99.5 Z range 約 {z_range:.3} m"));
        } else {
            reasons.push(format!("不是 2D：P0.5/P99.5 Z range 約 {z_range:.3} m"));
        }
    }
    if let Some(stats) = stats {
        if bbox_delta_is_large(stats.raw_bbox, stats.percentile_bbox) {
            reasons.push(
                "raw bbox 與 percentile bbox 差距很大，可能有 stray point / construction line"
                    .to_string(),
            );
        }
    }
    if reasons.is_empty() && inspect_status == "quarantined" {
        reasons.push("已被標記 quarantined，但目前缺少更細的 warning，需人工複核".to_string());
    }
    reasons
}

fn bbox_similarity(a: [f64; 6], b: [f64; 6]) -> f64 {
    let center_a = bbox_center(a);
    let center_b = bbox_center(b);
    let extent_a = bbox_extent(a);
    let extent_b = bbox_extent(b);
    let avg_diag = ((diag(extent_a) + diag(extent_b)) * 0.5).max(1.0);
    let center_dist = dist(center_a, center_b);
    let center_score = (1.0 - (center_dist / avg_diag).min(1.0)).clamp(0.0, 1.0);
    let extent_score = [0, 1, 2]
        .into_iter()
        .map(|idx| ratio_f64(extent_a[idx], extent_b[idx]))
        .sum::<f64>()
        / 3.0;
    center_score * 0.65 + extent_score * 0.35
}

fn ratio_score(a: u64, b: u64) -> f64 {
    if a == 0 && b == 0 {
        return 1.0;
    }
    if a == 0 || b == 0 {
        return 0.0;
    }
    let min = a.min(b) as f64;
    let max = a.max(b) as f64;
    min / max
}

fn ratio_f64(a: f64, b: f64) -> f64 {
    let a = a.abs();
    let b = b.abs();
    if a <= f64::EPSILON && b <= f64::EPSILON {
        return 1.0;
    }
    if a <= f64::EPSILON || b <= f64::EPSILON {
        return 0.0;
    }
    a.min(b) / a.max(b)
}

fn histogram_overlap(a: &BTreeMap<String, u64>, b: &BTreeMap<String, u64>) -> f64 {
    let total_a: u64 = a.values().sum();
    let total_b: u64 = b.values().sum();
    let denominator = total_a.max(total_b);
    if denominator == 0 {
        return 1.0;
    }
    let overlap: u64 = a
        .iter()
        .map(|(key, value)| value.min(b.get(key).unwrap_or(&0)))
        .sum();
    overlap as f64 / denominator as f64
}

fn bbox_delta_is_large(raw: [f64; 6], percentile: [f64; 6]) -> bool {
    let raw_diag = diag(bbox_extent(raw));
    let percentile_diag = diag(bbox_extent(percentile)).max(1.0);
    raw_diag / percentile_diag > 3.0
}

fn bbox_center(bbox: [f64; 6]) -> [f64; 3] {
    [
        (bbox[0] + bbox[3]) * 0.5,
        (bbox[1] + bbox[4]) * 0.5,
        (bbox[2] + bbox[5]) * 0.5,
    ]
}

fn bbox_extent(bbox: [f64; 6]) -> [f64; 3] {
    [
        (bbox[3] - bbox[0]).abs(),
        (bbox[4] - bbox[1]).abs(),
        (bbox[5] - bbox[2]).abs(),
    ]
}

fn diag(extent: [f64; 3]) -> f64 {
    (extent[0] * extent[0] + extent[1] * extent[1] + extent[2] * extent[2]).sqrt()
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn parse_bbox_json(text: &str) -> [f64; 6] {
    serde_json::from_str(text).unwrap_or([0.0; 6])
}

fn parse_json_map(text: &str) -> BTreeMap<String, u64> {
    serde_json::from_str(text).unwrap_or_default()
}

fn source_format_text(format: SourceFormat) -> &'static str {
    match format {
        SourceFormat::Ifc => "ifc",
        SourceFormat::Rvt => "rvt",
        SourceFormat::Dgn => "dgn",
        SourceFormat::Dwg => "dwg",
        SourceFormat::Unknown => "unknown",
    }
}

fn source_status_text(status: SourceStatus) -> &'static str {
    match status {
        SourceStatus::PendingInspect => "pending_inspect",
        SourceStatus::NeedsAlternativeRoute => "needs_alternative_route",
        SourceStatus::Approved => "approved",
        SourceStatus::Quarantined => "quarantined",
        SourceStatus::Converted => "converted",
        SourceStatus::Published => "published",
    }
}

fn status_counts(report: &InspectReviewReport) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for source in &report.sources {
        *counts.entry(source.inspect_status.clone()).or_insert(0) += 1;
    }
    counts
}

fn format_scale(scale: Option<f64>) -> String {
    scale
        .map(|value| {
            if (value.fract()).abs() < f64::EPSILON {
                format!("{value:.0}")
            } else {
                value.to_string()
            }
        })
        .unwrap_or_else(|| "-".to_string())
}

fn format_u64(value: u64) -> String {
    let text = value.to_string();
    let mut out = String::new();
    for (idx, ch) in text.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn format_bbox(bbox: Option<[f64; 6]>) -> String {
    bbox.map(|values| {
        values
            .into_iter()
            .map(|value| format!("{value:.3}"))
            .collect::<Vec<_>>()
            .join(", ")
    })
    .unwrap_or_else(|| "-".to_string())
}

fn format_duplicate_summary(source: &InspectReviewSource) -> String {
    source
        .duplicate_candidates
        .first()
        .map(|candidate| {
            format!(
                "{} {:.1}%",
                escape_html(&candidate.original_file_name),
                candidate.score * 100.0
            )
        })
        .unwrap_or_else(|| "-".to_string())
}

fn push_list(html: &mut String, values: &[String]) {
    html.push_str("<ul>");
    if values.is_empty() {
        html.push_str("<li class=\"muted\">none</li>");
    } else {
        for value in values {
            html.push_str(&format!("<li>{}</li>", escape_html(value)));
        }
    }
    html.push_str("</ul>");
}

fn push_histogram(html: &mut String, values: &BTreeMap<String, u64>, denominator: u64) {
    html.push_str("<ul>");
    if values.is_empty() {
        html.push_str("<li class=\"muted\">none</li>");
    } else {
        let mut pairs: Vec<_> = values.iter().collect();
        pairs.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        for (key, value) in pairs.into_iter().take(8) {
            let percent = if denominator > 0 {
                (*value as f64 / denominator as f64) * 100.0
            } else {
                0.0
            };
            html.push_str(&format!(
                "<li>{} <span class=\"muted\">{} · {:.1}%</span></li>",
                escape_html(key),
                format_u64(*value),
                percent
            ));
        }
    }
    html.push_str("</ul>");
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn escape_attr(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' => ch,
            _ => '_',
        })
        .collect()
}

fn chrono_like_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| format!("unix:{}", duration.as_secs()))
        .unwrap_or_else(|_| "unix:0".to_string())
}
