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
        render_publish_viewer_html_with_data(Some(&skeleton)),
    )
    .with_context(|| format!("寫入 publish viewer 失敗：{}", output.display()))?;
    update_review_report(input, output, &skeleton)?;
    Ok(())
}

pub fn render_publish_viewer_html() -> String {
    render_publish_viewer_html_with_data(None)
}

pub fn render_publish_viewer_html_with_data(skeleton: Option<&PublishSkeleton>) -> String {
    let embedded_sources = skeleton
        .and_then(|skeleton| serde_json::to_string(&skeleton.sources_manifest).ok())
        .unwrap_or_else(|| "null".to_string());
    let embedded_overlays = skeleton
        .and_then(|skeleton| serde_json::to_string(&skeleton.debug_overlays).ok())
        .unwrap_or_else(|| "null".to_string());
    let embedded = format!(
        r#"<script type="application/json" id="embeddedSourcesManifest">{}</script>
  <script type="application/json" id="embeddedDebugOverlays">{}</script>"#,
        script_safe_json(&embedded_sources),
        script_safe_json(&embedded_overlays)
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
    #missingCesium { display:none; position:absolute; inset:0; z-index:5; align-items:center; justify-content:center; padding:24px; text-align:center; background:#101418; color:#ffd2d2; }
    .btn { border:1px solid #3c4a57; background:#18222b; color:#e8eef5; border-radius:6px; padding:6px 10px; cursor:pointer; }
  </style>
  <script src="./Cesium-1.141/Build/Cesium/Cesium.js"></script>
</head>
<body>
  <div id="missingCesium">找不到同層 Cesium-1.141，請把 Cesium 放在 publish/Cesium-1.141 或改用正確相對路徑。</div>
  <div id="cesiumContainer"></div>
  <div id="toolbar">
    <label><input id="approvedToggle" type="checkbox" checked> approved only</label>
    <label><input id="rejectedToggle" type="checkbox"> rejected bbox</label>
    <label><input id="needsReviewToggle" type="checkbox"> needs review bbox</label>
    <button id="flyBtn" class="btn">Zoom</button>
  </div>
  <pre id="status">sources_manifest.json / debug_overlays.json</pre>
  <!--EMBEDDED_MANIFESTS-->
  <script>
    const DATA_FILES = {
      approved: "sources_manifest.json",
      overlays: "debug_overlays.json"
    };
    const state = { approved: [], rejected: [], needs_review: [], entities: [] };
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
    function bboxToRectangleEntity(item, decision) {
      if (!item.bbox_wgs84) return null;
      const b = item.bbox_wgs84;
      return {
        name: item.original_file_name,
        rectangle: {
          coordinates: Cesium.Rectangle.fromDegrees(b[0], b[1], b[3], b[4]),
          height: b[2],
          extrudedHeight: Math.max(b[5], b[2] + 1),
          material: fillColor(decision),
          outline: true,
          outlineColor: outlineColor(decision)
        },
        properties: {
          source_id: item.source_id,
          original_file_name: item.original_file_name,
          approval_decision: item.approval_decision,
          reason: item.reason,
          duplicate_of: item.duplicate_of || "",
          inspect_status: item.inspect_status
        }
      };
    }
    function readEmbeddedJson(id) {
      const node = document.getElementById(id);
      const text = node && node.textContent ? node.textContent.trim() : "";
      if (!text || text === "null") return null;
      return JSON.parse(text);
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
        const entityDef = bboxToRectangleEntity(item, decision);
        if (entityDef) state.entities.push(viewer.entities.add(entityDef));
      }
    }
    function refresh(viewer) {
      clearEntities(viewer);
      if (document.getElementById("approvedToggle").checked) addGroup(viewer, state.approved, "approved");
      if (document.getElementById("rejectedToggle").checked) addGroup(viewer, state.rejected, "rejected");
      if (document.getElementById("needsReviewToggle").checked) addGroup(viewer, state.needs_review, "needs_review");
      status([
        "basemap: EMAP5 WMTS",
        `approved only: ${state.approved.length}`,
        `rejected bbox: ${state.rejected.length}`,
        `needs review bbox: ${state.needs_review.length}`,
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
      state.approved = approved.sources || [];
      state.rejected = (overlays.sources || []).filter(s => s.approval_decision === "rejected");
      state.needs_review = (overlays.sources || []).filter(s => s.approval_decision === "needs_review");
      document.getElementById("approvedToggle").addEventListener("change", () => refresh(viewer));
      document.getElementById("rejectedToggle").addEventListener("change", () => refresh(viewer));
      document.getElementById("needsReviewToggle").addEventListener("change", () => refresh(viewer));
      document.getElementById("flyBtn").addEventListener("click", () => viewer.zoomTo(viewer.entities));
      refresh(viewer);
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

fn update_review_report(input: &Path, output: &Path, skeleton: &PublishSkeleton) -> Result<()> {
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
    let section = render_phase1f_review_section(output, skeleton);
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
