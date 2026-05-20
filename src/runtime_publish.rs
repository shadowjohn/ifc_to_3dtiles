use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};
use serde::Deserialize;

use crate::{
    crs::{enu_to_ecef_transform, project_to_wgs84},
    inspect_drilldown::{ApprovalManifest, ApprovalManifests},
    project::{ProjectManifest, SourceRecord},
    runtime_geometry::{RuntimeFeatureGeometry, build_runtime_proxy_glb},
    runtime_metadata::{
        RUNTIME_VERSION, RuntimeBudgetReport, RuntimeBudgetSource, RuntimeManifest,
        RuntimeManifestSource, RuntimeMetadataPayload, runtime_metadata_field_names,
        validate_runtime_metadata_fields,
    },
    spatial_pick::{
        SpatialPickFeatureInput, SpatialPickMetadataRef, SpatialPickSourceInput,
        build_spatial_pick_index,
    },
};

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeSourceBuildSummary {
    pub source_id: String,
    pub feature_count: usize,
    pub source_vertex_count: u64,
    pub bbox_percentile: Option<[f64; 6]>,
    pub origin_epsg3826: [f64; 3],
    pub origin_wgs84: [f64; 3],
    pub model_matrix: [f64; 16],
}

#[derive(Debug, Clone, PartialEq)]
struct RuntimeSourceStats {
    source_id: String,
    entity_count: u64,
    vertex_count: u64,
    percentile_bbox: Option<[f64; 6]>,
    selected_scale: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct SourceStatsRow {
    entity_count: u64,
    vertex_count: u64,
    percentile_bbox_json: String,
    selected_scale: Option<f64>,
}

pub fn write_runtime_publish_outputs(input: &Path, output: &Path) -> Result<()> {
    fs::create_dir_all(output)
        .with_context(|| format!("建立 runtime publish 目錄失敗：{}", output.display()))?;
    let manifest_path = input.join("source_manifest.json");
    let manifest: ProjectManifest = serde_json::from_slice(
        &fs::read(&manifest_path)
            .with_context(|| format!("讀取 source manifest 失敗：{}", manifest_path.display()))?,
    )
    .with_context(|| format!("解析 source manifest 失敗：{}", manifest_path.display()))?;
    let approvals = read_approval_manifests(&input.join("qa"))?;
    let db_path = input.join("project_inspect.db");
    let conn = Connection::open(&db_path)
        .with_context(|| format!("開啟 inspect DB 失敗：{}", db_path.display()))?;

    let source_map: HashMap<_, _> = manifest
        .sources
        .iter()
        .map(|source| (source.id.clone(), source))
        .collect();
    let rejected_or_review: BTreeMap<_, _> = approvals
        .rejected
        .sources
        .iter()
        .chain(approvals.needs_review.sources.iter())
        .map(|source| (source.source_id.clone(), source.decision.clone()))
        .collect();

    let runtime_root = output.join("runtime");
    let metadata_root = output.join("runtime_metadata");
    fs::create_dir_all(&runtime_root)
        .with_context(|| format!("建立 runtime 目錄失敗：{}", runtime_root.display()))?;
    fs::create_dir_all(&metadata_root).with_context(|| {
        format!(
            "建立 runtime metadata 目錄失敗：{}",
            metadata_root.display()
        )
    })?;

    let mut summaries = Vec::new();
    let mut budget_sources = Vec::new();
    let mut spatial_pick_sources = Vec::new();
    let mut spatial_pick_features = Vec::new();
    for approval in &approvals.approved.sources {
        if rejected_or_review.contains_key(&approval.source_id) {
            bail!(
                "source {} 同時出現在 approved 與 rejected/needs_review，停止 runtime publish",
                approval.source_id
            );
        }
        let source = source_map.get(&approval.source_id).with_context(|| {
            format!(
                "approved source 不在 source_manifest：{}",
                approval.source_id
            )
        })?;
        let stats = read_runtime_source_stats(&conn, &approval.source_id)?;
        let bbox = stats
            .percentile_bbox
            .or(source.percentile_bbox)
            .or(source.raw_bbox)
            .with_context(|| format!("approved source 缺少 bbox：{}", approval.source_id))?;
        let origin_epsg3826 = bbox_center(bbox);
        let lonlat =
            project_to_wgs84(manifest.source_epsg, origin_epsg3826[0], origin_epsg3826[1])?;
        let origin_wgs84 = [lonlat.lon_deg, lonlat.lat_deg, origin_epsg3826[2]];
        let model_matrix =
            enu_to_ecef_transform(lonlat.lon_deg, lonlat.lat_deg, origin_epsg3826[2]);
        let features = read_runtime_features(&conn, &approval.source_id)?;
        let runtime_output = build_runtime_proxy_glb(
            &approval.source_id,
            origin_epsg3826,
            origin_wgs84,
            &features,
        )?;
        spatial_pick_sources.push(SpatialPickSourceInput {
            source_id: approval.source_id.clone(),
            origin_epsg3826,
            origin_wgs84,
            model_matrix,
        });
        spatial_pick_features.extend(features.iter().map(|feature| SpatialPickFeatureInput {
            feature_id: feature.feature_id,
            source_id: feature.source_id.clone(),
            layer: feature.layer.clone(),
            name: None,
            category: if feature.geometry_type.trim().is_empty() {
                "UNKNOWN".to_string()
            } else {
                feature.geometry_type.trim().to_string()
            },
            bbox: Some(feature.bbox),
            metadata_ref: SpatialPickMetadataRef {
                global_id: None,
                express_id: Some(feature.feature_id),
            },
        }));
        let source_dir = runtime_root.join(&approval.source_id);
        fs::create_dir_all(&source_dir)
            .with_context(|| format!("建立 runtime source 目錄失敗：{}", source_dir.display()))?;
        fs::write(source_dir.join("runtime.glb"), &runtime_output.glb)
            .with_context(|| format!("寫入 runtime.glb 失敗：{}", source_dir.display()))?;
        write_runtime_metadata(
            source_dir.join("runtime_metadata.json"),
            &runtime_output.metadata,
        )?;
        write_runtime_metadata(
            metadata_root.join(format!("{}.json", approval.source_id)),
            &runtime_output.metadata,
        )?;
        fs::write(
            source_dir.join("runtime_pick.json"),
            serde_json::to_vec_pretty(&runtime_output.pick_index)?,
        )
        .with_context(|| format!("寫入 runtime_pick.json 失敗：{}", source_dir.display()))?;

        let geometry_file_size = runtime_output.glb.len() as u64;
        let runtime_metadata_bytes = serde_json::to_vec(&runtime_output.metadata)?.len();
        summaries.push(RuntimeSourceBuildSummary {
            source_id: approval.source_id.clone(),
            feature_count: runtime_output.metadata.features.len(),
            source_vertex_count: stats.vertex_count,
            bbox_percentile: stats.percentile_bbox.or(source.percentile_bbox),
            origin_epsg3826,
            origin_wgs84,
            model_matrix,
        });
        budget_sources.push(RuntimeBudgetSource {
            source_id: approval.source_id.clone(),
            triangle_count: runtime_output.triangle_count,
            vertex_count: runtime_output.mesh.positions.len(),
            runtime_metadata_bytes,
            bbox_volume: runtime_output.bbox_volume,
            geometry_file_size,
        });
        log::info!(
            "runtime source {}: {} features, {} proxy triangles, {} bytes",
            approval.source_id,
            runtime_output.metadata.features.len(),
            runtime_output.triangle_count,
            geometry_file_size
        );
    }

    let runtime_manifest = build_runtime_manifest(&manifest, &approvals, &summaries)?;
    let spatial_pick_index =
        build_spatial_pick_index("local", &spatial_pick_sources, &spatial_pick_features);
    fs::write(
        output.join("spatial_pick_index.json"),
        serde_json::to_vec_pretty(&spatial_pick_index)?,
    )
    .with_context(|| format!("寫入 spatial_pick_index.json 失敗：{}", output.display()))?;
    fs::write(
        output.join("runtime_manifest.json"),
        serde_json::to_vec_pretty(&runtime_manifest)?,
    )
    .with_context(|| format!("寫入 runtime_manifest.json 失敗：{}", output.display()))?;
    let budget = build_runtime_budget_report_with_pick_index(
        budget_sources,
        spatial_pick_index.features.len(),
        spatial_pick_index.warnings,
    );
    fs::write(
        output.join("runtime_budget_report.json"),
        serde_json::to_vec_pretty(&budget)?,
    )
    .with_context(|| format!("寫入 runtime_budget_report.json 失敗：{}", output.display()))?;

    Ok(())
}

pub fn build_runtime_manifest(
    manifest: &ProjectManifest,
    approvals: &ApprovalManifests,
    summaries: &[RuntimeSourceBuildSummary],
) -> Result<RuntimeManifest> {
    let summary_map: HashMap<_, _> = summaries
        .iter()
        .map(|summary| (summary.source_id.clone(), summary))
        .collect();
    let source_map: HashMap<_, _> = manifest
        .sources
        .iter()
        .map(|source| (source.id.clone(), source))
        .collect();
    let rejected_or_review: BTreeMap<_, _> = approvals
        .rejected
        .sources
        .iter()
        .chain(approvals.needs_review.sources.iter())
        .map(|source| (source.source_id.clone(), true))
        .collect();

    let mut sources = Vec::new();
    for approval in &approvals.approved.sources {
        if rejected_or_review.contains_key(&approval.source_id) {
            bail!(
                "source {} 同時出現在 approved 與 rejected/needs_review",
                approval.source_id
            );
        }
        let source = source_map.get(&approval.source_id).with_context(|| {
            format!(
                "approved source 不在 source_manifest：{}",
                approval.source_id
            )
        })?;
        let summary = summary_map
            .get(&approval.source_id)
            .with_context(|| format!("runtime source summary 不存在：{}", approval.source_id))?;
        sources.push(runtime_manifest_source(source, summary));
    }

    Ok(RuntimeManifest {
        runtime_version: RUNTIME_VERSION,
        approved_source_count: sources.len(),
        sources,
    })
}

pub fn build_runtime_budget_report(sources: Vec<RuntimeBudgetSource>) -> RuntimeBudgetReport {
    RuntimeBudgetReport {
        runtime_version: RUNTIME_VERSION,
        pick_index_generated: false,
        pick_index_feature_count: 0,
        pick_index_warnings: Vec::new(),
        sources,
    }
}

pub fn build_runtime_budget_report_with_pick_index(
    sources: Vec<RuntimeBudgetSource>,
    pick_index_feature_count: usize,
    pick_index_warnings: Vec<String>,
) -> RuntimeBudgetReport {
    RuntimeBudgetReport {
        runtime_version: RUNTIME_VERSION,
        pick_index_generated: true,
        pick_index_feature_count,
        pick_index_warnings,
        sources,
    }
}

fn runtime_manifest_source(
    source: &SourceRecord,
    summary: &RuntimeSourceBuildSummary,
) -> RuntimeManifestSource {
    RuntimeManifestSource {
        source_id: source.id.clone(),
        display_name: source.original_file_name.clone(),
        geometry_path: format!("runtime/{}/", source.id),
        status: "approved".to_string(),
        selected_scale: source.selected_scale,
        feature_count: summary.feature_count,
        vertex_count: summary.source_vertex_count,
        bbox_percentile: summary.bbox_percentile,
        runtime_metadata_fields: runtime_metadata_field_names(),
        model_matrix: summary.model_matrix,
        origin_epsg3826: summary.origin_epsg3826,
        origin_wgs84: summary.origin_wgs84,
    }
}

fn write_runtime_metadata(path: PathBuf, metadata: &RuntimeMetadataPayload) -> Result<()> {
    let value = serde_json::to_value(metadata)?;
    validate_runtime_metadata_fields(&value)?;
    fs::write(&path, serde_json::to_vec_pretty(metadata)?)
        .with_context(|| format!("寫入 runtime metadata 失敗：{}", path.display()))?;
    Ok(())
}

fn read_runtime_source_stats(conn: &Connection, source_id: &str) -> Result<RuntimeSourceStats> {
    let row = conn
        .query_row(
            "SELECT entity_count, vertex_count, percentile_bbox_json, selected_scale
             FROM source_stats
             WHERE source_id = ?1",
            params![source_id],
            |row| {
                Ok(SourceStatsRow {
                    entity_count: row.get(0)?,
                    vertex_count: row.get(1)?,
                    percentile_bbox_json: row.get(2)?,
                    selected_scale: row.get(3)?,
                })
            },
        )
        .with_context(|| format!("source_stats 找不到 approved source：{source_id}"))?;
    let percentile_bbox = parse_bbox_json(&row.percentile_bbox_json)?;
    Ok(RuntimeSourceStats {
        source_id: source_id.to_string(),
        entity_count: row.entity_count,
        vertex_count: row.vertex_count,
        percentile_bbox,
        selected_scale: row.selected_scale,
    })
}

fn read_runtime_features(
    conn: &Connection,
    source_id: &str,
) -> Result<Vec<RuntimeFeatureGeometry>> {
    let mut stmt = conn.prepare(
        "SELECT e.fid, COALESCE(e.layer, ''), COALESCE(e.geometry_type, ''),
                b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z
         FROM entities e
         JOIN entity_bboxes b ON b.source_id = e.source_id AND b.fid = e.fid
         WHERE e.source_id = ?1
         ORDER BY e.fid",
    )?;
    let rows = stmt.query_map(params![source_id], |row| {
        Ok(RuntimeFeatureGeometry {
            feature_id: row.get(0)?,
            source_id: source_id.to_string(),
            layer: row.get(1)?,
            geometry_type: row.get(2)?,
            material_id: "default".to_string(),
            bbox: [
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
            ],
        })
    })?;
    let mut features = Vec::new();
    for row in rows {
        features.push(row?);
    }
    if features.is_empty() {
        bail!("approved source 沒有可用 entity bbox：{source_id}");
    }
    Ok(features)
}

fn read_approval_manifests(path: &Path) -> Result<ApprovalManifests> {
    Ok(ApprovalManifests {
        approved: read_approval_manifest(&path.join("approved_sources.json"))?,
        rejected: read_approval_manifest(&path.join("rejected_sources.json"))?,
        needs_review: read_approval_manifest(&path.join("needs_review_sources.json"))?,
    })
}

fn read_approval_manifest(path: &Path) -> Result<ApprovalManifest> {
    serde_json::from_slice(
        &fs::read(path)
            .with_context(|| format!("讀取 approval manifest 失敗：{}", path.display()))?,
    )
    .with_context(|| format!("解析 approval manifest 失敗：{}", path.display()))
}

fn parse_bbox_json(text: &str) -> Result<Option<[f64; 6]>> {
    if text.trim().is_empty() || text.trim() == "null" {
        return Ok(None);
    }
    let value: serde_json::Value = serde_json::from_str(text)?;
    if value.is_null() {
        return Ok(None);
    }
    let bbox: [f64; 6] = serde_json::from_value(value)?;
    Ok(Some(bbox))
}

fn bbox_center(bbox: [f64; 6]) -> [f64; 3] {
    [
        (bbox[0] + bbox[3]) * 0.5,
        (bbox[1] + bbox[4]) * 0.5,
        (bbox[2] + bbox[5]) * 0.5,
    ]
}
