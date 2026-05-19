use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    cad_conversion::CadConversionReport,
    crs::project_to_wgs84,
    inspect_drilldown::{ApprovalManifests, ApprovalSourceDecision},
    project::{ProjectManifest, SourceFormat, SourceRecord},
    spatial_qa::{SpatialQaManifest, render_spatial_qa_review_summary, write_spatial_qa_manifest},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishSkeleton {
    pub sources_manifest: PublishSourcesManifest,
    pub debug_overlays: DebugOverlayManifest,
    pub normalized_sources: Vec<NormalizedSourceManifest>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishSourcesManifest {
    pub generated_at: String,
    pub project_id: String,
    pub source_epsg: u32,
    pub mode: String,
    pub sources: Vec<PublishSourceEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishSourceEntry {
    pub source_id: String,
    pub original_file_name: String,
    pub format: String,
    pub inspect_status: String,
    pub approval_decision: String,
    pub reason: String,
    pub duplicate_of: Option<String>,
    pub bbox: Option<[f64; 6]>,
    pub bbox_wgs84: Option<[f64; 6]>,
    pub normalized_manifest_path: PathBuf,
    pub converted_path: Option<PathBuf>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebugOverlayManifest {
    pub generated_at: String,
    pub source_epsg: u32,
    pub sources: Vec<DebugOverlaySource>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebugOverlaySource {
    pub source_id: String,
    pub original_file_name: String,
    pub format: String,
    pub inspect_status: String,
    pub approval_decision: String,
    pub reason: String,
    pub duplicate_of: Option<String>,
    pub bbox: Option<[f64; 6]>,
    pub bbox_wgs84: Option<[f64; 6]>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NormalizedSourceManifest {
    pub generated_at: String,
    pub source_id: String,
    pub original_file_name: String,
    pub format: String,
    pub inspect_status: String,
    pub approval_decision: String,
    pub reason: String,
    pub source_path: PathBuf,
    pub converted_path: Option<PathBuf>,
    pub bbox: Option<[f64; 6]>,
    pub bbox_wgs84: Option<[f64; 6]>,
    pub selected_scale: Option<f64>,
    pub fingerprint_hash: Option<String>,
    pub warnings: Vec<String>,
}

pub fn build_publish_skeleton(
    manifest: &ProjectManifest,
    approvals: &ApprovalManifests,
    converted_paths: &BTreeMap<String, PathBuf>,
) -> PublishSkeleton {
    let generated_at = chrono_like_now();
    let source_map: HashMap<_, _> = manifest
        .sources
        .iter()
        .map(|source| (source.id.clone(), source))
        .collect();
    let mut sources = Vec::new();
    let mut normalized_sources = Vec::new();
    for approval in &approvals.approved.sources {
        if let Some(source) = source_map.get(&approval.source_id) {
            let entry = publish_entry_from_source(
                &generated_at,
                source,
                approval,
                converted_paths.get(&approval.source_id).cloned(),
            );
            normalized_sources.push(normalized_manifest_from_source(
                &generated_at,
                source,
                approval,
                converted_paths.get(&approval.source_id).cloned(),
            ));
            sources.push(entry);
        }
    }

    let mut overlay_sources = Vec::new();
    for approval in approvals
        .rejected
        .sources
        .iter()
        .chain(approvals.needs_review.sources.iter())
    {
        if let Some(source) = source_map.get(&approval.source_id) {
            overlay_sources.push(debug_overlay_from_source(source, approval));
        }
    }

    PublishSkeleton {
        sources_manifest: PublishSourcesManifest {
            generated_at: generated_at.clone(),
            project_id: manifest.project_id.clone(),
            source_epsg: manifest.source_epsg,
            mode: "approved_only".to_string(),
            sources,
        },
        debug_overlays: DebugOverlayManifest {
            generated_at,
            source_epsg: manifest.source_epsg,
            sources: overlay_sources,
        },
        normalized_sources,
    }
}

pub fn write_publish_skeleton_outputs(input: &Path, output: &Path) -> Result<()> {
    fs::create_dir_all(output)
        .with_context(|| format!("建立 publish 輸出目錄失敗：{}", output.display()))?;
    let manifest_path = input.join("source_manifest.json");
    let manifest: ProjectManifest = serde_json::from_slice(
        &fs::read(&manifest_path)
            .with_context(|| format!("讀取 source manifest 失敗：{}", manifest_path.display()))?,
    )
    .with_context(|| format!("解析 source manifest 失敗：{}", manifest_path.display()))?;
    let approvals = read_approval_manifests(&input.join("qa"))?;
    let converted_paths = read_converted_paths(&input.join("cad_conversion_report.json"))?;
    let skeleton = build_publish_skeleton(&manifest, &approvals, &converted_paths);

    let project_root = output.parent().unwrap_or(input);
    let normalized_root = project_root.join("normalized");
    for normalized in &skeleton.normalized_sources {
        let source_dir = normalized_root.join(&normalized.source_id);
        fs::create_dir_all(&source_dir).with_context(|| {
            format!("建立 normalized source 目錄失敗：{}", source_dir.display())
        })?;
        fs::write(
            source_dir.join("source_manifest.json"),
            serde_json::to_vec_pretty(normalized)?,
        )
        .with_context(|| format!("寫入 normalized manifest 失敗：{}", source_dir.display()))?;
    }

    let spatial_qa = write_spatial_qa_manifest(input, output)?;

    fs::write(
        output.join("sources_manifest.json"),
        serde_json::to_vec_pretty(&skeleton.sources_manifest)?,
    )
    .with_context(|| format!("寫入 publish sources_manifest 失敗：{}", output.display()))?;
    fs::write(
        output.join("debug_overlays.json"),
        serde_json::to_vec_pretty(&skeleton.debug_overlays)?,
    )
    .with_context(|| format!("寫入 debug_overlays 失敗：{}", output.display()))?;
    fs::write(
        output.join("index.html"),
        render_publish_viewer_html_with_data_and_spatial(Some(&skeleton), Some(&spatial_qa)),
    )
    .with_context(|| format!("寫入 publish viewer 失敗：{}", output.display()))?;
    update_review_report(input, output, &skeleton, &spatial_qa)?;
    Ok(())
}

pub fn render_publish_viewer_html() -> String {
    render_publish_viewer_html_with_data(None)
}

pub fn render_publish_viewer_html_with_data(skeleton: Option<&PublishSkeleton>) -> String {
    render_publish_viewer_html_with_data_and_spatial(skeleton, None)
}

pub fn render_publish_viewer_html_with_data_and_spatial(
    skeleton: Option<&PublishSkeleton>,
    spatial_qa: Option<&SpatialQaManifest>,
) -> String {
    let embedded_sources = skeleton
        .and_then(|skeleton| serde_json::to_string(&skeleton.sources_manifest).ok())
        .unwrap_or_else(|| "null".to_string());
    let embedded_overlays = skeleton
        .and_then(|skeleton| serde_json::to_string(&skeleton.debug_overlays).ok())
        .unwrap_or_else(|| "null".to_string());
    let embedded_spatial_qa = spatial_qa
        .and_then(|spatial_qa| serde_json::to_string(spatial_qa).ok())
        .unwrap_or_else(|| "null".to_string());
    let embedded = format!(
        r#"<script type="application/json" id="embeddedSourcesManifest">{}</script>
  <script type="application/json" id="embeddedDebugOverlays">{}</script>
  <script type="application/json" id="embeddedSpatialQaManifest">{}</script>"#,
        script_safe_json(&embedded_sources),
        script_safe_json(&embedded_overlays),
        script_safe_json(&embedded_spatial_qa)
    );
    r##"<!doctype html>
<html lang="zh-Hant">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Approved Source Publish Skeleton</title>
  <link rel="stylesheet" href="./Cesium-1.141/Build/Cesium/Widgets/widgets.css">
  <style>
    html, body, #cesiumContainer { width:100%; height:100%; margin:0; overflow:hidden; background:#101418; color:#e8eef5; font-family:"Segoe UI", Arial, sans-serif; }
    #toolbar { position:absolute; z-index:2; top:12px; left:12px; background:rgba(16,20,24,.92); border:1px solid #2b3642; border-radius:8px; padding:10px 12px; display:flex; gap:12px; align-items:center; flex-wrap:wrap; }
    #toolbar label { display:flex; gap:6px; align-items:center; white-space:nowrap; }
    #status { position:absolute; z-index:2; right:12px; bottom:12px; max-width:520px; max-height:40vh; overflow:auto; background:rgba(16,20,24,.92); border:1px solid #2b3642; border-radius:8px; padding:10px 12px; white-space:pre-wrap; }
    #detailPanel { position:absolute; z-index:2; top:12px; right:12px; width:min(390px, calc(100vw - 24px)); max-height:calc(100vh - 96px); overflow:auto; background:rgba(16,20,24,.94); border:1px solid #2b3642; border-radius:8px; padding:12px; box-shadow:0 16px 40px rgba(0,0,0,.38); }
    #sourceListPanel { position:absolute; z-index:2; top:72px; left:12px; width:min(360px, calc(100vw - 24px)); max-height:calc(100vh - 150px); overflow:auto; background:rgba(16,20,24,.94); border:1px solid #2b3642; border-radius:8px; padding:12px; box-shadow:0 16px 40px rgba(0,0,0,.30); }
    #sourceListPanel h2 { margin:0 0 8px; font-size:15px; }
    #qaSearch { box-sizing:border-box; width:100%; margin:0 0 8px; padding:7px 9px; border:1px solid #334150; border-radius:6px; background:#0d141b; color:#e8eef5; }
    .source-row { width:100%; margin:0 0 6px; padding:8px; text-align:left; border:1px solid #2b3642; border-radius:6px; color:#e8eef5; background:#121a22; cursor:pointer; }
    .source-row:hover { border-color:#6aa7ff; background:#172330; }
    .source-title { display:flex; justify-content:space-between; gap:8px; align-items:center; }
    .source-meta { margin-top:4px; color:#91a0ad; font-size:12px; }
    .qa-actions { display:flex; gap:6px; flex-wrap:wrap; margin:8px 0; }
    .qa-actions .btn { font-size:12px; padding:5px 8px; }
    #detailPanel h2 { margin:0 0 8px; font-size:16px; }
    #detailPanel h3 { margin:12px 0 6px; font-size:13px; color:#dce8f3; }
    #detailPanel p { margin:4px 0; }
    #detailPanel ul { margin:4px 0 0 18px; padding:0; }
    #detailPanel li { margin:2px 0; }
    .muted { color:#91a0ad; }
    .mono { font-family:Consolas, "Cascadia Mono", monospace; font-size:12px; }
    .pill { display:inline-block; margin:0 4px 4px 0; padding:2px 7px; border:1px solid #3c4a57; border-radius:999px; background:#121a22; font-size:12px; }
    #missingCesium { display:none; position:absolute; inset:0; z-index:5; align-items:center; justify-content:center; padding:24px; text-align:center; background:#101418; color:#ffd2d2; }
    .btn { border:1px solid #3c4a57; background:#18222b; color:#e8eef5; border-radius:6px; padding:6px 10px; cursor:pointer; }
  </style>
  <script src="./Cesium-1.141/Build/Cesium/Cesium.js"></script>
</head>
<body>
  <div id="missingCesium">找不到同層 Cesium-1.141，請把 Cesium 放在 publish/Cesium-1.141 或改用正確相對路徑。</div>
  <div id="cesiumContainer"></div>
  <div id="toolbar">
    <label><input id="approvedGeometryToggle" type="checkbox"> approved geometry</label>
    <label><input id="approvedToggle" type="checkbox" checked> approved only</label>
    <label><input id="rejectedToggle" type="checkbox"> rejected bbox</label>
    <label><input id="needsReviewToggle" type="checkbox"> needs review bbox</label>
    <label><input id="percentileBboxToggle" type="checkbox" checked> percentile bbox</label>
    <label><input id="rawBboxToggle" type="checkbox"> raw bbox</label>
    <label><input id="aoiToggle" type="checkbox" checked> AOI</label>
    <label><input id="outlierToggle" type="checkbox"> outliers</label>
    <label><input id="duplicateCompareToggle" type="checkbox"> duplicate compare</label>
    <button id="flyBtn" class="btn">Zoom</button>
  </div>
  <aside id="detailPanel">
    <h2>Spatial QA</h2>
    <p class="muted">點 bbox / outlier marker 查看來源、狀態、scale、warnings、duplicate、top layers。</p>
    <div id="runtimeDebug" class="mono muted">runtime geometry unloaded</div>
  </aside>
  <aside id="sourceListPanel">
    <h2>Source QA</h2>
    <input id="qaSearch" type="search" placeholder="搜尋 source / status / layer">
    <div id="qaSummary" class="muted">loading</div>
    <div class="qa-actions">
      <button id="duplicateDrillBtn" class="btn">Duplicate</button>
      <button id="outlierListBtn" class="btn">Outliers</button>
    </div>
    <div id="sourceList"></div>
  </aside>
  <pre id="status">sources_manifest.json / debug_overlays.json</pre>
  <!--EMBEDDED_MANIFESTS-->
  <script>
    const DATA_FILES = {
      approved: "sources_manifest.json",
      overlays: "debug_overlays.json",
      spatialQa: "spatial_qa_manifest.json",
      runtimeManifest: "runtime_manifest.json"
    };
    const state = {
      approved: [],
      rejected: [],
      needs_review: [],
      spatialQa: null,
      sourceDetails: new Map(),
      entities: [],
      runtimeManifest: null,
      runtimeModels: [],
      runtimePickEntities: [],
      runtimeFeatureDetails: new Map(),
      runtimeLoaded: false,
      runtimeLoading: false,
      runtimeFeatureCount: 0,
      runtimeMetadataFields: []
    };
    function status(text) {
      document.getElementById("status").textContent = text;
    }
    function formatError(err) {
      if (!err) return "unknown error";
      if (err.stack) return err.stack;
      if (err.message) return err.message;
      try { return JSON.stringify(err); } catch (_) { return String(err); }
    }
    function cesiumMissing() {
      document.getElementById("missingCesium").style.display = "flex";
      status("Cesium-1.141 missing");
    }
    function fillColor(decision) {
      if (decision === "approved") return Cesium.Color.LIME.withAlpha(0.28);
      if (decision === "rejected") return Cesium.Color.RED.withAlpha(0.20);
      return Cesium.Color.YELLOW.withAlpha(0.22);
    }
    function outlineColor(decision) {
      if (decision === "approved") return Cesium.Color.LIME;
      if (decision === "rejected") return Cesium.Color.RED;
      return Cesium.Color.YELLOW;
    }
    function escapeHtml(value) {
      return String(value ?? "")
        .replaceAll("&", "&amp;")
        .replaceAll("<", "&lt;")
        .replaceAll(">", "&gt;")
        .replaceAll('"', "&quot;")
        .replaceAll("'", "&#39;");
    }
    function formatNumber(value) {
      if (value === null || value === undefined || !Number.isFinite(Number(value))) return "-";
      return Number(value).toLocaleString(undefined, { maximumFractionDigits: 3 });
    }
    function formatList(items, emptyText) {
      if (!items || !items.length) return `<p class="muted">${escapeHtml(emptyText || "none")}</p>`;
      return "<ul>" + items.map(item => `<li>${escapeHtml(item)}</li>`).join("") + "</ul>";
    }
    function formatCounts(items) {
      if (!items || !items.length) return `<p class="muted">none</p>`;
      return items.map(item => `<span class="pill">${escapeHtml(item.name)} ${formatNumber(item.count)}</span>`).join("");
    }
    function formatAoiGap(source) {
      if (!source || !source.aoi_gap_m) return "-";
      const labels = ["W", "S", "E", "N"];
      return source.aoi_gap_m.map((value, index) => `${labels[index]} ${formatNumber(value)}m`).join(" · ");
    }
    function formatRatio(value) {
      if (value === null || value === undefined || !Number.isFinite(Number(value))) return "-";
      return Number(value).toLocaleString(undefined, { maximumFractionDigits: 2 }) + "x";
    }
    function sourceSearchText(source) {
      return [
        source.source_id,
        source.original_file_name,
        source.inspect_status,
        source.approval_decision,
        source.aoi_status,
        ...(source.warnings || []),
        ...(source.quarantine_reasons || []),
        ...(source.top_layers || []).map(item => item.name),
        ...(source.geometry_types || []).map(item => item.name)
      ].join(" ").toLowerCase();
    }
    function renderSourceList() {
      const list = document.getElementById("sourceList");
      const query = (document.getElementById("qaSearch").value || "").trim().toLowerCase();
      const sources = (state.spatialQa?.sources || [])
        .filter(source => !query || sourceSearchText(source).includes(query));
      document.getElementById("qaSummary").textContent =
        `runtime ${state.approved.length} / debug ${state.rejected.length + state.needs_review.length} / showing ${sources.length}`;
      list.innerHTML = sources.map(source => `
        <button class="source-row" data-source-id="${escapeHtml(source.source_id)}">
          <span class="source-title">
            <b>${escapeHtml(source.original_file_name)}</b>
            <span class="pill">${escapeHtml(source.approval_decision)}</span>
          </span>
          <span class="source-meta">
            ${escapeHtml(source.inspect_status)} · ${escapeHtml(source.aoi_status || "no_bbox")} · scale ${escapeHtml(source.selected_scale ?? "-")}
          </span>
        </button>
      `).join("");
    }
    function propValue(entity, key) {
      const value = entity && entity.properties && entity.properties[key];
      if (!value) return undefined;
      return typeof value.getValue === "function" ? value.getValue(Cesium.JulianDate.now()) : value;
    }
    function bboxForKind(source, kind) {
      if (!source) return null;
      if (kind === "raw") return source.raw_bbox_wgs84;
      return source.percentile_bbox_wgs84 || source.raw_bbox_wgs84;
    }
    function bboxToRectangleEntity(item, decision, kind) {
      const detail = state.sourceDetails.get(item.source_id) || item;
      const b = bboxForKind(detail, kind || "percentile") || item.bbox_wgs84;
      if (!b) return null;
      const isRaw = kind === "raw";
      return {
        name: `${item.original_file_name} ${isRaw ? "raw" : "percentile"} bbox`,
        rectangle: {
          coordinates: Cesium.Rectangle.fromDegrees(b[0], b[1], b[3], b[4]),
          height: b[2],
          extrudedHeight: Math.max(b[5], b[2] + 1),
          material: (isRaw ? outlineColor(decision).withAlpha(0.08) : fillColor(decision)),
          outline: true,
          outlineColor: outlineColor(decision)
        },
        properties: {
          qa_kind: "source",
          source_id: item.source_id,
          bbox_kind: isRaw ? "raw" : "percentile",
          original_file_name: item.original_file_name,
          approval_decision: item.approval_decision,
          reason: item.reason,
          duplicate_of: item.duplicate_of || "",
          inspect_status: item.inspect_status
        }
      };
    }
    function aoiEntity() {
      const aoi = state.spatialQa && state.spatialQa.aoi;
      if (!aoi || !aoi.wgs84_bbox) return null;
      const b = aoi.wgs84_bbox;
      return {
        name: "EPSG:3826 AOI",
        rectangle: {
          coordinates: Cesium.Rectangle.fromDegrees(b[0], b[1], b[2], b[3]),
          material: Cesium.Color.CYAN.withAlpha(0.06),
          outline: true,
          outlineColor: Cesium.Color.CYAN
        },
        properties: { qa_kind: "aoi" }
      };
    }
    function duplicateCompareEntities() {
      const entities = [];
      const pairs = state.spatialQa?.duplicate_pairs || [];
      for (const pair of pairs) {
        const a = pair.source_a_percentile_bbox_wgs84;
        const b = pair.source_b_percentile_bbox_wgs84;
        if (a) {
          entities.push({
            name: `${pair.source_a_name} duplicate compare`,
            rectangle: {
              coordinates: Cesium.Rectangle.fromDegrees(a[0], a[1], a[3], a[4]),
              height: a[2],
              extrudedHeight: Math.max(a[5], a[2] + 1),
              material: Cesium.Color.LIME.withAlpha(0.12),
              outline: true,
              outlineColor: Cesium.Color.LIME
            },
            properties: { qa_kind: "duplicate", source_id: pair.source_a_id, pair_source_id: pair.source_b_id }
          });
        }
        if (b) {
          entities.push({
            name: `${pair.source_b_name} duplicate compare`,
            rectangle: {
              coordinates: Cesium.Rectangle.fromDegrees(b[0], b[1], b[3], b[4]),
              height: b[2],
              extrudedHeight: Math.max(b[5], b[2] + 1),
              material: Cesium.Color.RED.withAlpha(0.12),
              outline: true,
              outlineColor: Cesium.Color.RED
            },
            properties: { qa_kind: "duplicate", source_id: pair.source_b_id, pair_source_id: pair.source_a_id }
          });
        }
      }
      return entities;
    }
    function outlierEntity(outlier) {
      if (!outlier.center_wgs84) return null;
      const c = outlier.center_wgs84;
      return {
        name: `${outlier.original_file_name} FID ${outlier.fid}`,
        position: Cesium.Cartesian3.fromDegrees(c[0], c[1], c[2] || 0),
        point: {
          pixelSize: 10,
          color: Cesium.Color.MAGENTA.withAlpha(0.92),
          outlineColor: Cesium.Color.WHITE,
          outlineWidth: 1
        },
        properties: {
          qa_kind: "outlier",
          source_id: outlier.source_id,
          fid: outlier.fid,
          outlier_index: outlier._index
        }
      };
    }
    function readEmbeddedJson(id) {
      const node = document.getElementById(id);
      const text = node && node.textContent ? node.textContent.trim() : "";
      if (!text || text === "null") return null;
      return JSON.parse(text);
    }
    function rectangleFromBbox(bbox) {
      if (!bbox) return null;
      return Cesium.Rectangle.fromDegrees(bbox[0], bbox[1], bbox[3], bbox[4]);
    }
    function zoomToSource(sourceId, kind) {
      const source = state.sourceDetails.get(sourceId);
      if (!source || !window.viewer) return;
      const bbox = bboxForKind(source, kind || "percentile");
      const rectangle = rectangleFromBbox(bbox);
      if (rectangle) {
        window.viewer.camera.flyTo({ destination: rectangle, duration: 0.55 });
      }
      showSourceDetail(sourceId, kind || "percentile");
    }
    function zoomToOutlier(outlier) {
      if (!outlier || !outlier.center_wgs84 || !window.viewer) return;
      const c = outlier.center_wgs84;
      window.viewer.camera.flyTo({
        destination: Cesium.Cartesian3.fromDegrees(c[0], c[1], (c[2] || 0) + 1500),
        duration: 0.55
      });
      showOutlierDetail(outlier);
    }
    function showDuplicateDetail(index) {
      const pair = (state.spatialQa?.duplicate_pairs || [])[index || 0];
      if (!pair) return;
      document.getElementById("duplicateCompareToggle").checked = true;
      refresh(window.viewer);
      document.getElementById("detailPanel").innerHTML = `
        <h2>Duplicate Compare</h2>
        <p><b>score</b> ${(pair.score * 100).toFixed(1)}%</p>
        <p><b>retain</b> ${escapeHtml(pair.source_a_id === pair.retain_source_id ? pair.source_a_name : pair.source_b_name)}</p>
        <p><b>reject</b> ${escapeHtml(pair.source_a_id === pair.reject_source_id ? pair.source_a_name : pair.source_b_name)}</p>
        <p>${escapeHtml(pair.recommendation_reason)}</p>
        <div class="qa-actions">
          <button class="btn" onclick="zoomToSource('${escapeHtml(pair.source_a_id)}', 'percentile')">${escapeHtml(pair.source_a_name)}</button>
          <button class="btn" onclick="zoomToSource('${escapeHtml(pair.source_b_id)}', 'percentile')">${escapeHtml(pair.source_b_name)}</button>
        </div>
        <p class="mono">entity ${formatNumber(pair.entity_count_a)} / ${formatNumber(pair.entity_count_b)}</p>
        <p class="mono">vertex ${formatNumber(pair.vertex_count_a)} / ${formatNumber(pair.vertex_count_b)}</p>
      `;
      if (window.viewer && state.entities.length) window.viewer.zoomTo(window.viewer.entities);
    }
    function showOutlierList() {
      const outliers = (state.spatialQa?.outliers || []).slice(0, 16);
      document.getElementById("outlierToggle").checked = true;
      refresh(window.viewer);
      document.getElementById("detailPanel").innerHTML = `
        <h2>Outlier Markers</h2>
        <p class="muted">依 Phase 1E score 排序，點 FID 可定位。</p>
        ${outliers.map((outlier, index) => `
          <button class="source-row" onclick="zoomToOutlier((state.spatialQa?.outliers || [])[${index}])">
            <span class="source-title"><b>FID ${escapeHtml(outlier.fid)}</b><span class="pill">${escapeHtml(outlier.layer)}</span></span>
            <span class="source-meta">${escapeHtml(outlier.reason)} · score ${formatNumber(outlier.score)}</span>
          </button>
        `).join("")}
      `;
    }
    function showSourceDetail(sourceId, titleSuffix) {
      const source = state.sourceDetails.get(sourceId);
      const panel = document.getElementById("detailPanel");
      if (!source) {
        panel.innerHTML = `<h2>Spatial QA</h2><p class="muted">找不到 source detail：${escapeHtml(sourceId)}</p>`;
        return;
      }
      const duplicateItems = (source.duplicate_candidates || [])
        .map(candidate => `${candidate.original_file_name} ${(candidate.score * 100).toFixed(1)}%`);
      panel.innerHTML = `
        <h2>${escapeHtml(source.original_file_name)} ${escapeHtml(titleSuffix || "")}</h2>
        <p><b>source</b> <span class="mono">${escapeHtml(source.source_id)}</span></p>
        <p><b>status</b> ${escapeHtml(source.inspect_status)} / ${escapeHtml(source.approval_decision)}</p>
        <p><b>scale</b> ${source.selected_scale ?? "-"}</p>
        <p><b>entities</b> ${formatNumber(source.entity_count)} · <b>vertices</b> ${formatNumber(source.vertex_count)}</p>
        <p><b>duplicate_of</b> <span class="mono">${escapeHtml(source.duplicate_of || "-")}</span></p>
        <p><b>AOI</b> ${escapeHtml(source.aoi_status || "no_bbox")} · ${escapeHtml(formatAoiGap(source))}</p>
        <p><b>bbox inflation</b> ${escapeHtml(formatRatio(source.bbox_inflation_ratio))}</p>
        <div class="qa-actions">
          <button class="btn" onclick="zoomToSource('${escapeHtml(source.source_id)}', 'percentile')">Zoom P0.5/P99.5</button>
          <button class="btn" onclick="zoomToSource('${escapeHtml(source.source_id)}', 'raw')">Zoom Raw</button>
        </div>
        <h3>Warnings</h3>${formatList(source.warnings, "none")}
        <h3>Quarantine</h3>${formatList(source.quarantine_reasons, "none")}
        <h3>Duplicate</h3>${formatList(duplicateItems, "none")}
        <h3>Top Layers</h3>${formatCounts(source.top_layers)}
        <h3>Geometry Types</h3>${formatCounts(source.geometry_types)}
        <h3>BBox</h3>
        <p class="mono">raw: ${escapeHtml(JSON.stringify(source.raw_bbox || null))}</p>
        <p class="mono">P0.5/P99.5: ${escapeHtml(JSON.stringify(source.percentile_bbox || null))}</p>
      `;
    }
    function showOutlierDetail(outlier) {
      const panel = document.getElementById("detailPanel");
      panel.innerHTML = `
        <h2>Outlier FID ${escapeHtml(outlier.fid)}</h2>
        <p><b>source</b> ${escapeHtml(outlier.original_file_name)} <span class="mono">${escapeHtml(outlier.source_id)}</span></p>
        <p><b>layer</b> ${escapeHtml(outlier.layer)}</p>
        <p><b>handle</b> <span class="mono">${escapeHtml(outlier.entity_handle || "-")}</span></p>
        <p><b>type</b> ${escapeHtml(outlier.geometry_type || "-")}</p>
        <p><b>reason</b> ${escapeHtml(outlier.reason)}</p>
        <p><b>score</b> ${formatNumber(outlier.score)}</p>
        <p><b>distance AOI</b> ${formatNumber(outlier.distance_from_aoi)} m</p>
        <p><b>distance source center</b> ${formatNumber(outlier.distance_from_source_center)} m</p>
        <p class="mono">bbox: ${escapeHtml(JSON.stringify(outlier.bbox))}</p>
      `;
    }
    function showAoiDetail() {
      const aoi = state.spatialQa?.aoi;
      if (!aoi) return;
      document.getElementById("detailPanel").innerHTML = `
        <h2>AOI</h2>
        <p><b>EPSG</b> ${aoi.epsg}</p>
        <p class="mono">EPSG:3826 ${escapeHtml(JSON.stringify(aoi.epsg3826_bbox))}</p>
        <p class="mono">WGS84 ${escapeHtml(JSON.stringify(aoi.wgs84_bbox))}</p>
      `;
    }
    function createEmap5WmtsProvider() {
      const provider = new Cesium.UrlTemplateImageryProvider({
        url: "https://wmts.nlsc.gov.tw/wmts/EMAP5/default/GoogleMapsCompatible/{z}/{y}/{x}",
        maximumLevel: 19,
        credit: "NLSC EMAP5"
      });
      provider.errorEvent.addEventListener((error) => {
        error.retry = false;
        status("EMAP5 WMTS 載入失敗；bbox QA viewer 仍可使用。");
      });
      return provider;
    }
    function setupWmtsBasemap(viewer) {
      try {
        viewer.imageryLayers.addImageryProvider(createEmap5WmtsProvider(), 0);
      } catch (err) {
        status("EMAP5 WMTS 初始化失敗\n" + formatError(err));
      }
    }
    function clearEntities(viewer) {
      for (const entity of state.entities) viewer.entities.remove(entity);
      state.entities = [];
    }
    function addGroup(viewer, items, decision) {
      for (const item of items) {
        if (document.getElementById("percentileBboxToggle").checked) {
          const entityDef = bboxToRectangleEntity(item, decision, "percentile");
          if (entityDef) state.entities.push(viewer.entities.add(entityDef));
        }
        if (document.getElementById("rawBboxToggle").checked) {
          const entityDef = bboxToRectangleEntity(item, decision, "raw");
          if (entityDef) state.entities.push(viewer.entities.add(entityDef));
        }
      }
    }
    function refresh(viewer) {
      clearEntities(viewer);
      if (document.getElementById("aoiToggle").checked) {
        const entityDef = aoiEntity();
        if (entityDef) state.entities.push(viewer.entities.add(entityDef));
      }
      if (document.getElementById("approvedToggle").checked) addGroup(viewer, state.approved, "approved");
      if (document.getElementById("rejectedToggle").checked) addGroup(viewer, state.rejected, "rejected");
      if (document.getElementById("needsReviewToggle").checked) addGroup(viewer, state.needs_review, "needs_review");
      if (document.getElementById("duplicateCompareToggle").checked) {
        for (const entityDef of duplicateCompareEntities()) state.entities.push(viewer.entities.add(entityDef));
      }
      if (document.getElementById("outlierToggle").checked) {
        (state.spatialQa?.outliers || []).forEach((outlier, index) => {
          outlier._index = index;
          const entityDef = outlierEntity(outlier);
          if (entityDef) state.entities.push(viewer.entities.add(entityDef));
        });
      }
      setRuntimeVisible(document.getElementById("approvedGeometryToggle").checked && state.runtimeLoaded);
      status([
        "basemap: EMAP5 WMTS",
        `approved geometry: ${state.runtimeLoaded ? state.runtimeModels.length : 0}`,
        `approved only: ${state.approved.length}`,
        `rejected bbox: ${state.rejected.length}`,
        `needs review bbox: ${state.needs_review.length}`,
        `outlier marker: ${(state.spatialQa?.outliers || []).length}`,
        "metadata: source_id, original_file_name, approval_decision, reason, duplicate_of"
      ].join("\n"));
    }
    async function loadJson(url, embeddedId) {
      const embedded = readEmbeddedJson(embeddedId);
      if (embedded) return embedded;
      if (location.protocol === "file:") {
        throw new Error(`${url} 沒有內嵌資料；請重新執行 Phase 1F publish skeleton，或改用本機 HTTP server 開啟。`);
      }
      return Cesium.Resource.fetchJson({ url });
    }
    async function fetchRuntimeJson(url) {
      if (location.protocol === "file:") {
        throw new Error("runtime geometry 需要透過本機 HTTP server 載入，請使用 tools/run_phase1f_publish_viewer.ps1。");
      }
      return Cesium.Resource.fetchJson({ url: `${url}?t=${Date.now()}` });
    }
    function runtimeDebugText() {
      const sourceCount = state.runtimeManifest?.sources?.length || 0;
      return [
        `runtime source count: ${sourceCount}`,
        `loaded geometry count: ${state.runtimeModels.length}`,
        `runtime feature count: ${state.runtimeFeatureCount}`,
        `runtime metadata fields: ${state.runtimeMetadataFields.join(", ") || "-"}`
      ].join("\n");
    }
    function updateRuntimeDebug() {
      const node = document.getElementById("runtimeDebug");
      if (node) node.textContent = runtimeDebugText();
    }
    function setRuntimeVisible(visible) {
      for (const entry of state.runtimeModels) entry.model.show = visible;
      for (const entity of state.runtimePickEntities) entity.show = visible;
      updateRuntimeDebug();
    }
    function runtimeMaterial(feature, alpha) {
      const color = feature.explode_group_key && feature.explode_group_key.includes("電梯")
        ? Cesium.Color.ORANGE
        : Cesium.Color.CYAN;
      return color.withAlpha(alpha);
    }
    function clearRuntimeHighlight() {
      for (const entity of state.runtimePickEntities) {
        const feature = state.runtimeFeatureDetails.get(String(propValue(entity, "runtime_feature_key")));
        entity.box.material = runtimeMaterial(feature || {}, 0.025);
        entity.box.outline = false;
      }
      for (const entry of state.runtimeModels) {
        entry.model.color = Cesium.Color.WHITE;
        entry.model.colorBlendMode = Cesium.ColorBlendMode.MIX;
        entry.model.colorBlendAmount = 0.0;
      }
    }
    function highlightRuntimeSource(sourceId) {
      clearRuntimeHighlight();
      for (const entity of state.runtimePickEntities) {
        if (propValue(entity, "source_id") === sourceId) {
          entity.box.material = Cesium.Color.YELLOW.withAlpha(0.42);
          entity.box.outline = true;
          entity.box.outlineColor = Cesium.Color.YELLOW;
        }
      }
      for (const entry of state.runtimeModels) {
        if (entry.source_id === sourceId) {
          entry.model.color = Cesium.Color.YELLOW.withAlpha(0.45);
          entry.model.colorBlendMode = Cesium.ColorBlendMode.HIGHLIGHT;
          entry.model.colorBlendAmount = 0.45;
        }
      }
    }
    function highlightRuntimeGroup(explodeGroupKey) {
      clearRuntimeHighlight();
      for (const entity of state.runtimePickEntities) {
        if (propValue(entity, "explode_group_key") === explodeGroupKey) {
          entity.box.material = Cesium.Color.YELLOW.withAlpha(0.48);
          entity.box.outline = true;
          entity.box.outlineColor = Cesium.Color.YELLOW;
        }
      }
    }
    function showRuntimeFeatureDetail(feature) {
      if (!feature) return;
      window.currentRuntimeFeature = feature;
      highlightRuntimeGroup(feature.explode_group_key);
      document.getElementById("detailPanel").innerHTML = `
        <h2>Runtime Feature</h2>
        <p><b>source_id</b> <span class="mono">${escapeHtml(feature.source_id)}</span></p>
        <p><b>feature_id</b> <span class="mono">${escapeHtml(feature.feature_id)}</span></p>
        <p><b>explode_group_key</b> ${escapeHtml(feature.explode_group_key)}</p>
        <p><b>ifc_type</b> ${escapeHtml(feature.ifc_type)}</p>
        <p><b>material_id</b> ${escapeHtml(feature.material_id)}</p>
        <div class="qa-actions">
          <button class="btn" onclick="highlightRuntimeSource(window.currentRuntimeFeature.source_id)">Highlight Source</button>
          <button class="btn" onclick="highlightRuntimeGroup(window.currentRuntimeFeature.explode_group_key)">Highlight Group</button>
        </div>
        <div id="runtimeDebug" class="mono muted">${escapeHtml(runtimeDebugText())}</div>
      `;
    }
    function runtimeBoxEntity(feature) {
      if (!feature.center_wgs84 || !feature.dimensions) return null;
      const center = feature.center_wgs84;
      const dimensions = feature.dimensions.map(value => Math.max(Number(value) || 0.25, 0.25));
      const key = `${feature.source_id}:${feature.feature_id}`;
      state.runtimeFeatureDetails.set(key, feature);
      return {
        name: `runtime ${feature.source_id} FID ${feature.feature_id}`,
        position: Cesium.Cartesian3.fromDegrees(center[0], center[1], center[2] || 0),
        box: {
          dimensions: new Cesium.Cartesian3(dimensions[0], dimensions[1], dimensions[2]),
          material: runtimeMaterial(feature, 0.025),
          outline: false,
          outlineColor: Cesium.Color.YELLOW
        },
        properties: {
          qa_kind: "runtime_feature",
          runtime_feature_key: key,
          source_id: feature.source_id,
          feature_id: feature.feature_id,
          explode_group_key: feature.explode_group_key,
          ifc_type: feature.ifc_type,
          material_id: feature.material_id
        }
      };
    }
    async function loadRuntimeGeometry(viewer) {
      if (state.runtimeLoaded || state.runtimeLoading) {
        setRuntimeVisible(true);
        return;
      }
      state.runtimeLoading = true;
      try {
        state.runtimeManifest = await fetchRuntimeJson(DATA_FILES.runtimeManifest);
        state.runtimeFeatureCount = 0;
        state.runtimeMetadataFields = [];
        for (const source of (state.runtimeManifest.sources || [])) {
          const modelMatrix = Cesium.Matrix4.fromArray(source.model_matrix);
          const url = `${source.geometry_path}runtime.glb`;
          const model = Cesium.Model.fromGltfAsync
            ? await Cesium.Model.fromGltfAsync({ url, modelMatrix, allowPicking: true })
            : Cesium.Model.fromGltf({ url, modelMatrix, allowPicking: true });
          if (!Cesium.Model.fromGltfAsync && model.readyPromise) await model.readyPromise;
          model.show = true;
          viewer.scene.primitives.add(model);
          state.runtimeModels.push({ source_id: source.source_id, model });
          const pickIndex = await fetchRuntimeJson(`${source.geometry_path}runtime_pick.json`);
          for (const feature of (pickIndex.features || [])) {
            const entityDef = runtimeBoxEntity(feature);
            if (!entityDef) continue;
            const entity = viewer.entities.add(entityDef);
            state.runtimePickEntities.push(entity);
          }
          state.runtimeFeatureCount += Number(source.feature_count || 0);
          state.runtimeMetadataFields = source.runtime_metadata_fields || state.runtimeMetadataFields;
        }
        state.runtimeLoaded = true;
        setRuntimeVisible(true);
        updateRuntimeDebug();
      } catch (err) {
        document.getElementById("approvedGeometryToggle").checked = false;
        status("runtime geometry 載入失敗\n" + formatError(err));
      } finally {
        state.runtimeLoading = false;
      }
    }
    async function toggleRuntimeGeometry(viewer) {
      if (document.getElementById("approvedGeometryToggle").checked) {
        await loadRuntimeGeometry(viewer);
      } else {
        setRuntimeVisible(false);
      }
    }
    function setupPickHandler(viewer) {
      const handler = new Cesium.ScreenSpaceEventHandler(viewer.scene.canvas);
      handler.setInputAction((movement) => {
        const picked = viewer.scene.pick(movement.position);
        const entity = picked && picked.id;
        if (!entity) return;
        const kind = propValue(entity, "qa_kind");
        if (kind === "source" || kind === "duplicate") {
          showSourceDetail(propValue(entity, "source_id"), propValue(entity, "bbox_kind"));
        } else if (kind === "outlier") {
          const index = Number(propValue(entity, "outlier_index"));
          const outlier = state.spatialQa?.outliers?.[index];
          if (outlier) showOutlierDetail(outlier);
        } else if (kind === "aoi") {
          showAoiDetail();
        } else if (kind === "runtime_feature") {
          const key = String(propValue(entity, "runtime_feature_key"));
          showRuntimeFeatureDetail(state.runtimeFeatureDetails.get(key));
        }
      }, Cesium.ScreenSpaceEventType.LEFT_CLICK);
      return handler;
    }
    function wireReviewNavigation(viewer) {
      document.getElementById("qaSearch").addEventListener("input", renderSourceList);
      document.getElementById("sourceList").addEventListener("click", (event) => {
        const row = event.target.closest(".source-row");
        if (!row) return;
        zoomToSource(row.dataset.sourceId, "percentile");
      });
      document.getElementById("duplicateDrillBtn").addEventListener("click", () => showDuplicateDetail(0));
      document.getElementById("outlierListBtn").addEventListener("click", showOutlierList);
      window.zoomToSource = zoomToSource;
      window.zoomToOutlier = zoomToOutlier;
      window.showDuplicateDetail = showDuplicateDetail;
      window.showOutlierList = showOutlierList;
      window.highlightRuntimeSource = highlightRuntimeSource;
      window.highlightRuntimeGroup = highlightRuntimeGroup;
    }
    async function main() {
      if (!window.Cesium) { cesiumMissing(); return; }
      const viewer = new Cesium.Viewer("cesiumContainer", {
        animation: false,
        timeline: false,
        baseLayer: false,
        imageryProvider: false,
        terrainProvider: new Cesium.EllipsoidTerrainProvider(),
        baseLayerPicker: false,
        geocoder: false,
        homeButton: true,
        sceneModePicker: false,
        navigationHelpButton: false,
        showRenderLoopErrors: false
      });
      window.viewer = viewer;
      viewer.scene.renderError.addEventListener((_scene, error) => {
        status("Cesium render error\n" + formatError(error));
      });
      if (viewer.scene.globe) {
        viewer.scene.globe.baseColor = Cesium.Color.fromCssColorString("#0b1118");
      }
      setupWmtsBasemap(viewer);
      const approved = await loadJson(DATA_FILES.approved, "embeddedSourcesManifest");
      const overlays = await loadJson(DATA_FILES.overlays, "embeddedDebugOverlays");
      state.spatialQa = await loadJson(DATA_FILES.spatialQa, "embeddedSpatialQaManifest");
      for (const source of (state.spatialQa?.sources || [])) {
        state.sourceDetails.set(source.source_id, source);
      }
      state.approved = approved.sources || [];
      state.rejected = (overlays.sources || []).filter(s => s.approval_decision === "rejected");
      state.needs_review = (overlays.sources || []).filter(s => s.approval_decision === "needs_review");
      document.getElementById("approvedGeometryToggle").addEventListener("change", () => toggleRuntimeGeometry(viewer));
      document.getElementById("approvedToggle").addEventListener("change", () => refresh(viewer));
      document.getElementById("rejectedToggle").addEventListener("change", () => refresh(viewer));
      document.getElementById("needsReviewToggle").addEventListener("change", () => refresh(viewer));
      document.getElementById("percentileBboxToggle").addEventListener("change", () => refresh(viewer));
      document.getElementById("rawBboxToggle").addEventListener("change", () => refresh(viewer));
      document.getElementById("aoiToggle").addEventListener("change", () => refresh(viewer));
      document.getElementById("outlierToggle").addEventListener("change", () => refresh(viewer));
      document.getElementById("duplicateCompareToggle").addEventListener("change", () => refresh(viewer));
      document.getElementById("flyBtn").addEventListener("click", () => viewer.zoomTo(viewer.entities));
      setupPickHandler(viewer);
      wireReviewNavigation(viewer);
      renderSourceList();
      refresh(viewer);
      if (state.approved[0]) showSourceDetail(state.approved[0].source_id, "approved");
      if (state.entities.length) viewer.zoomTo(viewer.entities);
    }
    main().catch(err => status(String(err && err.stack || err)));
  </script>
</body>
</html>
"##
    .replace("<!--EMBEDDED_MANIFESTS-->", &embedded)
    .to_string()
}

fn publish_entry_from_source(
    generated_at: &str,
    source: &SourceRecord,
    approval: &ApprovalSourceDecision,
    converted_path: Option<PathBuf>,
) -> PublishSourceEntry {
    let normalized =
        normalized_manifest_from_source(generated_at, source, approval, converted_path.clone());
    PublishSourceEntry {
        source_id: normalized.source_id,
        original_file_name: normalized.original_file_name,
        format: normalized.format,
        inspect_status: normalized.inspect_status,
        approval_decision: normalized.approval_decision,
        reason: normalized.reason,
        duplicate_of: approval.duplicate_of.clone(),
        bbox: normalized.bbox,
        bbox_wgs84: normalized.bbox_wgs84,
        normalized_manifest_path: PathBuf::from("..")
            .join("normalized")
            .join(&source.id)
            .join("source_manifest.json"),
        converted_path,
        warnings: normalized.warnings,
    }
}

fn normalized_manifest_from_source(
    generated_at: &str,
    source: &SourceRecord,
    approval: &ApprovalSourceDecision,
    converted_path: Option<PathBuf>,
) -> NormalizedSourceManifest {
    let bbox = source.percentile_bbox.or(source.raw_bbox);
    let mut warnings = source.warnings.clone();
    let bbox_wgs84 = bbox.and_then(|bbox| match bbox_to_wgs84(bbox) {
        Ok(value) => Some(value),
        Err(err) => {
            warnings.push(format!("bbox WGS84 projection unavailable: {err}"));
            None
        }
    });
    if bbox.is_none() {
        warnings.push("no bbox available; viewer will not draw this source".to_string());
    }
    NormalizedSourceManifest {
        generated_at: generated_at.to_string(),
        source_id: source.id.clone(),
        original_file_name: source.original_file_name.clone(),
        format: source_format_text(source.format).to_string(),
        inspect_status: source
            .inspect_status
            .clone()
            .unwrap_or_else(|| source_status_text(source.status).to_string()),
        approval_decision: approval.decision.clone(),
        reason: approval.reason.clone(),
        source_path: source.path.clone(),
        converted_path,
        bbox,
        bbox_wgs84,
        selected_scale: source.selected_scale,
        fingerprint_hash: source.fingerprint_hash.clone(),
        warnings,
    }
}

fn debug_overlay_from_source(
    source: &SourceRecord,
    approval: &ApprovalSourceDecision,
) -> DebugOverlaySource {
    let normalized = normalized_manifest_from_source(&chrono_like_now(), source, approval, None);
    DebugOverlaySource {
        source_id: normalized.source_id,
        original_file_name: normalized.original_file_name,
        format: normalized.format,
        inspect_status: normalized.inspect_status,
        approval_decision: normalized.approval_decision,
        reason: normalized.reason,
        duplicate_of: approval.duplicate_of.clone(),
        bbox: normalized.bbox,
        bbox_wgs84: normalized.bbox_wgs84,
        warnings: normalized.warnings,
    }
}

fn read_approval_manifests(qa_dir: &Path) -> Result<ApprovalManifests> {
    Ok(ApprovalManifests {
        approved: read_json(&qa_dir.join("approved_sources.json"))?,
        rejected: read_json(&qa_dir.join("rejected_sources.json"))?,
        needs_review: read_json(&qa_dir.join("needs_review_sources.json"))?,
    })
}

fn read_converted_paths(path: &Path) -> Result<BTreeMap<String, PathBuf>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let report: CadConversionReport = read_json(path)?;
    Ok(report
        .entries
        .into_iter()
        .filter_map(|entry| entry.converted_path.map(|path| (entry.source_id, path)))
        .collect())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    serde_json::from_slice(
        &fs::read(path).with_context(|| format!("讀取 JSON 失敗：{}", path.display()))?,
    )
    .with_context(|| format!("解析 JSON 失敗：{}", path.display()))
}

fn bbox_to_wgs84(bbox: [f64; 6]) -> Result<[f64; 6]> {
    let min = project_to_wgs84(3826, bbox[0], bbox[1])?;
    let max = project_to_wgs84(3826, bbox[3], bbox[4])?;
    Ok([
        min.lon_deg.min(max.lon_deg),
        min.lat_deg.min(max.lat_deg),
        bbox[2],
        min.lon_deg.max(max.lon_deg),
        min.lat_deg.max(max.lat_deg),
        bbox[5],
    ])
}

fn update_review_report(
    input: &Path,
    output: &Path,
    skeleton: &PublishSkeleton,
    spatial_qa: &SpatialQaManifest,
) -> Result<()> {
    let path = input.join("review_report.html");
    if !path.exists() {
        return Ok(());
    }
    let html = fs::read_to_string(&path)
        .with_context(|| format!("讀取 review_report 失敗：{}", path.display()))?;
    let marker = "<h2>Phase 1F Publish Skeleton</h2>";
    let clean = html
        .split(marker)
        .next()
        .unwrap_or(&html)
        .trim_end_matches("</main></body></html>")
        .to_string();
    let section = format!(
        "{}{}",
        render_phase1f_review_section(output, skeleton),
        render_spatial_qa_review_summary(
            "publish/spatial_qa_manifest.json",
            &spatial_qa.aoi,
            spatial_qa.sources.len(),
            spatial_qa.duplicate_pairs.len(),
            spatial_qa.outliers.len(),
        )
    );
    fs::write(path, format!("{clean}{section}</main></body></html>"))
        .with_context(|| "寫入 Phase 1F review_report summary 失敗".to_string())?;
    Ok(())
}

fn render_phase1f_review_section(output: &Path, skeleton: &PublishSkeleton) -> String {
    format!(
        r#"<h2>Phase 1F Publish Skeleton</h2><div class="grid"><div class="box"><b>Publish gate</b><ul><li>approved runtime sources：{}</li><li>debug overlay sources：{}</li><li>publish folder：{}</li></ul></div><div class="box"><b>Viewer</b><ul><li><a href="publish/index.html">publish/index.html</a></li><li>sources_manifest.json</li><li>debug_overlays.json</li></ul></div></div>"#,
        skeleton.sources_manifest.sources.len(),
        skeleton.debug_overlays.sources.len(),
        escape_html(&output.display().to_string())
    )
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

fn source_status_text(status: crate::project::SourceStatus) -> &'static str {
    match status {
        crate::project::SourceStatus::PendingInspect => "pending_inspect",
        crate::project::SourceStatus::NeedsAlternativeRoute => "needs_alternative_route",
        crate::project::SourceStatus::Approved => "approved",
        crate::project::SourceStatus::Quarantined => "quarantined",
        crate::project::SourceStatus::Converted => "converted",
        crate::project::SourceStatus::Published => "published",
    }
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn script_safe_json(text: &str) -> String {
    text.replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
}

fn chrono_like_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| format!("unix:{}", duration.as_secs()))
        .unwrap_or_else(|_| "unix:0".to_string())
}
