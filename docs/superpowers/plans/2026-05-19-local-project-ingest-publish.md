# Local Project Ingest And Publish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a local-first project workspace that can inspect many IFC/DGN/DWG/RVT sources, detect CRS/scale problems, quarantine suspicious files, dump CAD hierarchy metadata, enrich metadata/group candidates, and publish one easy-to-load 3D Tiles package with flat / 90 / 180 normal modes.

**Architecture:** This is a Local Web Platform / BIM-GIS workstation, not a Rust GUI app. Rust is the trusted controller and pipeline orchestrator, not a full CAD kernel. External tools handle DGN/DWG conversion and probing, SQLite stores the durable property/index database, and Cesium is the core GIS/BIM viewer. Keep the existing Rust IFC/RVT converter as the geometry core, then add an ingest layer before conversion and a publish layer after conversion. The ingest layer writes deterministic manifests and reports; each approved source is converted into its own normalized tileset folder first, and the publish layer only creates root tilesets that reference those per-source tilesets. Do not merge source geometry at the start; source separation is required for large-project debugging, duplicate detection, quarantine, filtering, and group explosion. Every approved source is normalized into one canonical world space: `EPSG:3826` meters, local project ENU, Z-up.

**Tech Stack:** Rust CLI/core worker, optional Rust Axum local API, optional PHP dashboard adapter for existing 3wa-style deployment, serde JSON manifests, SQLite property/index database, existing IFC STEP parser, optional GDAL/OGR/ODA CLI probes for DGN/DWG inspection and conversion, PowerShell helper scripts, Bootstrap 5 + jQuery + GoldenLayout + Tabulator + jsTree, Cesium production viewer, Three.js debug GLB viewer.

---

## Scope

This plan intentionally does not build a cloud platform yet. It builds a local project workspace and local web/viewer-friendly outputs first, because the hard problem is data trust: CRS, scale, duplicates, wrong-country geometry, 2D drawings, missing materials, and group semantics.

Implementation priority is split into two levels:

- **Level 1, required:** CRS, scale, centroid/percentile bounds, source identity, source transform, CAD hierarchy dump, group candidates, group spatial centers, material/color metadata, property metadata, duplicate fingerprints, quarantine decisions, viewer debug metadata, and per-source publish structure must be stable before chasing perfect geometry.
- **Level 2, follow-up:** perfect DGN geometry, perfect hierarchy reconstruction, parametric solids, smart objects, civil alignments, terrain semantics, and OpenRoads metadata are best-effort unless a high-quality official/exported source is available.

The reason is practical: a GIS viewer is damaged more by a model in the wrong country, duplicated on itself, or at the wrong scale than by imperfect material fidelity. Position correctness, source traceability, and duplicate suppression are release gates.

Out of scope for this phase:

- Direct Bentley DGN kernel implementation in Rust.
- Paid Bentley / MicroStation / OpenRoads automation.
- Cloud job queue or multi-user database.
- Draco / meshopt compression.
- Replacing Cesium production viewer.
- Rust GUI, Qt Desktop, or Electron all-in-one app packaging for the first platform.

The intended operator experience is a browser-based dockable workstation, closer to QGIS / Navisworks / VS Code than a normal website. Cesium must keep the largest screen area. Panels are for source tree, group tree, metadata, warnings, quarantine queue, SQL, logs/progress, and CRS/AOI debug overlays.

## File Structure

- Create `src/project.rs`: project workspace layout, manifest structs, source status enums.
- Create `src/inspect.rs`: inspect IFC/DGN/DWG/RVT files and produce per-source raw bbox, percentile bounds, CRS, unit scale candidates, warnings.
- Create `src/cad_metadata.rs`: DGN/DWG hierarchy dump schema for models, references, levels, cells, shared cells, attachments, element classes, materials, and line styles.
- Create `src/georef.rs`: canonical CRS normalization, source transform, scale detection, centroid/percentile AOI validation logic for EPSG:3826, `scale=1000.0/1.0/0.1/0.01/0.001`.
- Create `src/fingerprint.rs`: geometry fingerprint and duplicate candidate detection by vertex count, triangle count, bbox, and surface area.
- Create `src/grouping.rs`: derive group candidates from IFC metadata and future DGN/DWG dumps.
- Create `src/properties_db.rs`: SQLite schema and writers for source, feature, group, material, transform, warning, and duplicate records.
- Create `src/publish.rs`: wrap approved per-source normalized tilesets into one root publish folder with three normal-mode root tilesets plus `sources_manifest.json`, `groups.json`, and warnings.
- Modify `src/main.rs`: add subcommands `inspect`, `convert-source`, `publish`, keep current direct conversion behavior.
- Future create `src/server.rs`: Axum local API for project/source/feature/group/quarantine operations after inspect reports are trustworthy.
- Modify `src/lib.rs`: export new modules.
- Modify `src/convert.rs`: expose per-source conversion hooks and include new source/group fields in metadata.
- Create `tests/project_pipeline.rs`: manifest/georef/group/publish tests.
- Create `tools/inspect_cad_sources.ps1`: optional local helper that checks availability of `ogrinfo`, `ogr2ogr`, ODA/other CLI tools without requiring them.
- Create `docs/local_project_workflow.md`: operator workflow and failure handling.
- Future create `docs/local_web_workstation.md`: Cesium-first workstation UI layout and API/PHP dashboard contract.
- Modify `.gitignore`: ignore `sample_files/`, `*.dgn`, `*.dwg`, and generated project workspaces.

---

### Task 0: Site Inventory And Freeze Baseline

**Files:**
- Create: `tools/inspect_cad_sources.ps1`
- Modify: `.gitignore`
- Modify: `history.md`

This task freezes the current machine and sample-file state before implementation. Do not start geometry conversion or publish work until this baseline is recorded.

- [ ] **Step 1: Ignore local CAD/BIM source drops**

Modify `.gitignore` so project delivery files stay local:

```gitignore
*.dgn
*.dwg
*.dxf
/sample_files/
```

- [ ] **Step 2: Create baseline CAD probe script**

Create `tools/inspect_cad_sources.ps1`:

```powershell
param(
  [Alias("Input")]
  [string]$InputPath = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\sample_files\淡江大橋移交模型",
  [string]$Output = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\cad_probe"
)

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $Output | Out-Null

function Get-ToolInfo {
  param([string[]]$Names)

  foreach ($name in $Names) {
    $cmd = Get-Command $name -ErrorAction SilentlyContinue
    if ($cmd) {
      $version = $null
      try {
        $version = (Get-Item -LiteralPath $cmd.Source).VersionInfo.FileVersion
      } catch {
        $version = $null
      }

      return [ordered]@{
        found = $true
        name = $cmd.Name
        source = $cmd.Source
        version = $version
      }
    }
  }

  return [ordered]@{
    found = $false
    name = $Names[0]
    source = $null
    version = $null
  }
}

function Get-OdaVersionRisk {
  param([string]$Version)

  if (-not $Version) {
    return "unknown_version"
  }

  try {
    $parsed = [version]$Version
    if ($parsed.Major -lt 26) {
      return "too_old_for_2026_cad_delivery"
    }
    return "acceptable_baseline"
  } catch {
    return "unparseable_version"
  }
}

if (-not (Test-Path -LiteralPath $InputPath)) {
  throw "輸入目錄不存在：$InputPath"
}

$files = Get-ChildItem -LiteralPath $InputPath -Recurse -File |
  Sort-Object FullName |
  Select-Object FullName, Extension, Length

$extensionDistribution = $files |
  Group-Object { if ($_.Extension) { $_.Extension.ToLowerInvariant() } else { "[no_extension]" } } |
  Sort-Object -Property @{ Expression = "Count"; Descending = $true }, @{ Expression = "Name"; Ascending = $true } |
  ForEach-Object {
    [ordered]@{
      extension = $_.Name
      count = $_.Count
      total_bytes = ($_.Group | Measure-Object -Property Length -Sum).Sum
    }
  }

$cadFiles = $files | Where-Object { $_.Extension -match '^\.(dgn|dwg|dxf)$' }
$tools = [ordered]@{
  ogrinfo = Get-ToolInfo @("ogrinfo", "ogrinfo.exe")
  ogr2ogr = Get-ToolInfo @("ogr2ogr", "ogr2ogr.exe")
  oda_file_converter = Get-ToolInfo @("ODAFileConverter", "ODAFileConverter.exe")
}
$tools.oda_file_converter["version_risk"] = Get-OdaVersionRisk $tools.oda_file_converter.version

$report = [ordered]@{
  input = $InputPath
  output = $Output
  generated_at = (Get-Date).ToString("o")
  tools = $tools
  file_count = $files.Count
  cad_file_count = $cadFiles.Count
  extension_distribution = @($extensionDistribution)
  cad_files = @($cadFiles)
  note = "這支 probe 只盤點本機工具與檔案分布，不執行轉檔，也不需要 Bentley 付費工具。"
}

$reportPath = Join-Path $Output "cad_probe_report.json"
$report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $reportPath -Encoding UTF8

Write-Host "CAD probe report: $reportPath"
Write-Host "Tools:"
foreach ($toolName in $report.tools.Keys) {
  $tool = $report.tools[$toolName]
  if ($tool.found) {
    Write-Host ("  {0}: FOUND - {1}" -f $toolName, $tool.source)
  } else {
    Write-Host ("  {0}: MISSING" -f $toolName)
  }
}

Write-Host ("Sample files: {0}, CAD files: {1}" -f $report.file_count, $report.cad_file_count)
Write-Host "Extension distribution:"
foreach ($item in $report.extension_distribution) {
  Write-Host ("  {0}: {1} files, {2} bytes" -f $item.extension, $item.count, $item.total_bytes)
}
```

- [ ] **Step 3: Freeze git baseline**

Run:

```powershell
git status --short --branch
git log --oneline -5
```

Expected:

- Branch is `main`.
- The newest commit is recorded in `history.md`.
- `sample_files/` must not appear as a tracked change after `.gitignore` is updated.

- [ ] **Step 4: Freeze Rust test baseline**

Run exactly:

```powershell
cargo test
```

Expected:

- If `cargo` is available, tests pass before Task 1 starts.
- If `cargo` is missing, record this as a Task 0 blocker in `history.md`; install or repair Rust before implementing Task 1.

- [ ] **Step 5: Freeze CAD tool and sample-file baseline**

Run:

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\inspect_cad_sources.ps1
```

Expected:

- `out\cad_probe\cad_probe_report.json` exists.
- Report includes `ogrinfo`, `ogr2ogr`, and `oda_file_converter` availability.
- Report includes `file_count`, `cad_file_count`, and extension distribution.
- ODA File Converter versions below major `26` are marked `too_old_for_2026_cad_delivery`.

- [ ] **Step 6: Commit**

```powershell
git add .gitignore tools/inspect_cad_sources.ps1 history.md docs/superpowers/plans/2026-05-19-local-project-ingest-publish.md
git commit -m "Add baseline inventory task"
```

---

### Task 1: Project Workspace Manifest

**Files:**
- Create: `src/project.rs`
- Modify: `src/lib.rs`
- Test: `tests/project_pipeline.rs`

- [ ] **Step 1: Write failing tests for workspace manifest defaults**

Add to `tests/project_pipeline.rs`:

```rust
use ifc_to_3dtiles::project::{
    ProjectManifest, SourceFormat, SourceRecord, SourceStatus, WorkspaceLayout,
};
use std::path::PathBuf;

#[test]
fn workspace_layout_uses_predictable_folders() {
    let layout = WorkspaceLayout::new(PathBuf::from(r"C:\work\tamkang_bridge"));
    assert_eq!(layout.sources, PathBuf::from(r"C:\work\tamkang_bridge\sources"));
    assert_eq!(layout.staging, PathBuf::from(r"C:\work\tamkang_bridge\staging"));
    assert_eq!(layout.normalized, PathBuf::from(r"C:\work\tamkang_bridge\normalized"));
    assert_eq!(layout.publish, PathBuf::from(r"C:\work\tamkang_bridge\publish"));
}

#[test]
fn source_record_preserves_source_identity() {
    let source = SourceRecord {
        id: "dgn-djb-m-su-monitor".to_string(),
        path: PathBuf::from(r"sources\DJB-M-SU-監測.dgn.i.dgn"),
        format: SourceFormat::Dgn,
        status: SourceStatus::PendingInspect,
        original_size_bytes: 170_884_000,
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
    };
    assert_eq!(source.format, SourceFormat::Dgn);
    assert_eq!(source.status, SourceStatus::PendingInspect);
}

#[test]
fn project_manifest_serializes_to_stable_json() {
    let manifest = ProjectManifest {
        project_id: "tamkang_bridge".to_string(),
        source_epsg: 3826,
        anchor_source_id: None,
        allowed_scales: vec![1000.0, 1.0, 0.1, 0.01, 0.001],
        sources: vec![],
    };
    let json = serde_json::to_string_pretty(&manifest).expect("serialize manifest");
    assert!(json.contains("\"source_epsg\": 3826"));
    assert!(json.contains("\"allowed_scales\""));
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```powershell
cargo test workspace_layout_uses_predictable_folders source_record_preserves_source_identity project_manifest_serializes_to_stable_json
```

Expected: fail because `ifc_to_3dtiles::project` does not exist.

- [ ] **Step 3: Implement manifest structs**

Create `src/project.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceLayout {
    pub root: PathBuf,
    pub sources: PathBuf,
    pub staging: PathBuf,
    pub normalized: PathBuf,
    pub publish: PathBuf,
}

impl WorkspaceLayout {
    pub fn new(root: PathBuf) -> Self {
        Self {
            sources: root.join("sources"),
            staging: root.join("staging"),
            normalized: root.join("normalized"),
            publish: root.join("publish"),
            root,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceFormat {
    Ifc,
    Rvt,
    Dgn,
    Dwg,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceStatus {
    PendingInspect,
    Approved,
    Quarantined,
    Converted,
    Published,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceRecord {
    pub id: String,
    pub path: PathBuf,
    pub format: SourceFormat,
    pub status: SourceStatus,
    pub original_size_bytes: u64,
    pub detected_crs: Option<String>,
    pub unit_scale_to_meter: Option<f64>,
    pub anchor_distance_m: Option<f64>,
    pub raw_bbox: Option<[f64; 6]>,
    pub percentile_bbox: Option<[f64; 6]>,
    pub transform: Option<serde_json::Value>,
    pub cad_metadata_path: Option<PathBuf>,
    pub fingerprint_hash: Option<String>,
    pub duplicate_candidates: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub project_id: String,
    pub source_epsg: u32,
    pub anchor_source_id: Option<String>,
    pub allowed_scales: Vec<f64>,
    pub sources: Vec<SourceRecord>,
}
```

Modify `src/lib.rs`:

```rust
pub mod b3dm;
pub mod convert;
pub mod crs;
pub mod geometry;
pub mod glb;
pub mod model;
pub mod project;
pub mod revit;
pub mod rvt;
pub mod rvt_job;
pub mod step;
pub mod tiles;
```

- [ ] **Step 4: Run tests and verify they pass**

Run:

```powershell
cargo test workspace_layout_uses_predictable_folders source_record_preserves_source_identity project_manifest_serializes_to_stable_json
```

Expected: all three tests pass.

- [ ] **Step 5: Commit**

```powershell
git add src/project.rs src/lib.rs tests/project_pipeline.rs
git commit -m "Add local project workspace manifest"
```

---

### Task 2: Source File Discovery And Inspect Report

**Files:**
- Create: `src/inspect.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs`
- Test: `tests/project_pipeline.rs`

- [ ] **Step 1: Write failing tests for file discovery**

Add to `tests/project_pipeline.rs`:

```rust
use ifc_to_3dtiles::inspect::{discover_sources, source_format_from_path};
use ifc_to_3dtiles::project::SourceFormat;
use std::fs;

#[test]
fn source_format_detects_supported_extensions() {
    assert_eq!(source_format_from_path("a.ifc".as_ref()), SourceFormat::Ifc);
    assert_eq!(source_format_from_path("a.rvt".as_ref()), SourceFormat::Rvt);
    assert_eq!(source_format_from_path("a.dgn".as_ref()), SourceFormat::Dgn);
    assert_eq!(source_format_from_path("a.dwg".as_ref()), SourceFormat::Dwg);
    assert_eq!(source_format_from_path("a.txt".as_ref()), SourceFormat::Unknown);
}

#[test]
fn discover_sources_skips_unknown_files_and_keeps_large_cad_out_of_git() {
    let temp = tempfile::tempdir().expect("tempdir");
    fs::write(temp.path().join("bridge.ifc"), "ISO-10303-21;").expect("ifc");
    fs::write(temp.path().join("tower.dgn"), "fake").expect("dgn");
    fs::write(temp.path().join("readme.txt"), "skip").expect("txt");

    let sources = discover_sources(temp.path()).expect("discover");
    let formats: Vec<_> = sources.iter().map(|s| s.format).collect();
    assert_eq!(formats, vec![SourceFormat::Ifc, SourceFormat::Dgn]);
    assert!(sources.iter().all(|s| s.status == ifc_to_3dtiles::project::SourceStatus::PendingInspect));
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```powershell
cargo test source_format_detects_supported_extensions discover_sources_skips_unknown_files_and_keeps_large_cad_out_of_git
```

Expected: fail because `inspect` module does not exist.

- [ ] **Step 3: Implement discovery**

Create `src/inspect.rs`:

```rust
use crate::project::{SourceFormat, SourceRecord, SourceStatus};
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn source_format_from_path(path: &Path) -> SourceFormat {
    match path.extension().and_then(|ext| ext.to_str()).map(str::to_ascii_lowercase) {
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
        let metadata = fs::metadata(&path)
            .with_context(|| format!("讀取檔案資訊失敗：{}", path.display()))?;
        let id = stable_source_id(root, &path);
        sources.push(SourceRecord {
            id,
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
    for entry in fs::read_dir(root).with_context(|| format!("讀取目錄失敗：{}", root.display()))? {
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
    relative
        .to_string_lossy()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
```

Modify `src/lib.rs`:

```rust
pub mod inspect;
```

- [ ] **Step 4: Add CLI subcommand skeleton**

Modify `src/main.rs` to wrap current direct conversion into a `convert` path while preserving existing flags. Add enum:

```rust
#[derive(Debug, Clone, clap::Subcommand)]
enum Command {
    Inspect {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 3826)]
        source_epsg: u32,
    },
}
```

Keep backward compatibility by leaving existing options on `Cli`; only add `command: Option<Command>`. In `main()`, before direct conversion:

```rust
if let Some(Command::Inspect { input, output, source_epsg }) = cli.command {
    fs::create_dir_all(&output)
        .with_context(|| format!("建立 inspect 輸出目錄失敗：{}", output.display()))?;
    let sources = ifc_to_3dtiles::inspect::discover_sources(&input)?;
    let manifest = ifc_to_3dtiles::project::ProjectManifest {
        project_id: safe_stem(&input),
        source_epsg,
        anchor_source_id: sources.iter().find(|s| s.format == ifc_to_3dtiles::project::SourceFormat::Ifc).map(|s| s.id.clone()),
        allowed_scales: vec![1000.0, 1.0, 0.1, 0.01, 0.001],
        sources,
    };
    let path = output.join("source_manifest.json");
    fs::write(&path, serde_json::to_vec_pretty(&manifest)?)
        .with_context(|| format!("寫入 manifest 失敗：{}", path.display()))?;
    println!("{}", path.display());
    return Ok(());
}
```

- [ ] **Step 5: Run tests and CLI smoke test**

Run:

```powershell
cargo test source_format_detects_supported_extensions discover_sources_skips_unknown_files_and_keeps_large_cad_out_of_git
cargo run -- inspect `
  --input "C:\Users\stw_s\Desktop\ifc_to_3dtiles\sample_files\淡江大橋移交模型" `
  --output "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\inspect_tamkang" `
  --source-epsg 3826
```

Expected:

- Tests pass.
- `out\inspect_tamkang\source_manifest.json` exists.
- Manifest contains `.ifc`, `.dgn`, `.dwg`; no conversion output is created yet.

- [ ] **Step 6: Commit**

```powershell
git add src/inspect.rs src/lib.rs src/main.rs tests/project_pipeline.rs
git commit -m "Add source discovery inspect command"
```

---

### Task 3: Canonical Georef, Scale Classification, And Source Transform

**Files:**
- Create: `src/georef.rs`
- Modify: `src/lib.rs`
- Modify: `src/inspect.rs`
- Test: `tests/project_pipeline.rs`

- [ ] **Step 1: Write failing tests for scale selection, percentile AOI checks, and source transform**

Add to `tests/project_pipeline.rs`:

```rust
use ifc_to_3dtiles::georef::{
    Aoi, Bounds2, BoundsSummary, SourceTransform, classify_source_scale,
};

#[test]
fn scale_classifier_accepts_taiwan_epsg_3826_meter_bounds() {
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let bounds = Bounds2::new(300_000.0, 2_787_000.0, 301_000.0, 2_788_000.0);
    let summary = BoundsSummary::from_raw_and_percentile(bounds, bounds);
    let result = classify_source_scale(&summary, &aoi, &[1000.0, 1.0, 0.1, 0.01, 0.001]);
    assert_eq!(result.selected_scale, Some(1.0));
    assert_eq!(result.status, "inside_aoi");
}

#[test]
fn scale_classifier_detects_centimeter_like_coordinates() {
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let bounds = Bounds2::new(30_000_000.0, 278_700_000.0, 30_100_000.0, 278_800_000.0);
    let summary = BoundsSummary::from_raw_and_percentile(bounds, bounds);
    let result = classify_source_scale(&summary, &aoi, &[1000.0, 1.0, 0.1, 0.01, 0.001]);
    assert_eq!(result.selected_scale, Some(0.01));
    assert_eq!(result.status, "inside_aoi");
}

#[test]
fn scale_classifier_detects_millimeter_like_coordinates() {
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let bounds = Bounds2::new(300_000_000.0, 2_787_000_000.0, 301_000_000.0, 2_788_000_000.0);
    let summary = BoundsSummary::from_raw_and_percentile(bounds, bounds);
    let result = classify_source_scale(&summary, &aoi, &[1000.0, 1.0, 0.1, 0.01, 0.001]);
    assert_eq!(result.selected_scale, Some(0.001));
    assert_eq!(result.status, "inside_aoi");
}

#[test]
fn scale_classifier_uses_percentile_bounds_when_raw_bbox_has_stray_points() {
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let raw_bounds = Bounds2::new(-9_000_000.0, -9_000_000.0, 9_000_000.0, 9_000_000.0);
    let percentile_bounds = Bounds2::new(300_000.0, 2_787_000.0, 301_000.0, 2_788_000.0);
    let summary = BoundsSummary::from_raw_and_percentile(raw_bounds, percentile_bounds);
    let result = classify_source_scale(&summary, &aoi, &[1000.0, 1.0, 0.1, 0.01, 0.001]);
    assert_eq!(result.selected_scale, Some(1.0));
    assert!(result.warnings.iter().any(|w| w.contains("raw bbox")));
}

#[test]
fn scale_classifier_quarantines_far_away_model() {
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let bounds = Bounds2::new(4_000_000.0, 6_000_000.0, 4_100_000.0, 6_100_000.0);
    let summary = BoundsSummary::from_raw_and_percentile(bounds, bounds);
    let result = classify_source_scale(&summary, &aoi, &[1000.0, 1.0, 0.1, 0.01, 0.001]);
    assert_eq!(result.selected_scale, None);
    assert_eq!(result.status, "outside_aoi");
    assert!(result.warnings.iter().any(|w| w.contains("outside AOI")));
}

#[test]
fn source_transform_declares_canonical_space() {
    let transform = SourceTransform::identity("EPSG:3826", 1.0);
    assert_eq!(transform.canonical_crs, "EPSG:3826");
    assert_eq!(transform.canonical_space, "EPSG:3826 meters / local ENU / Z-up");
    assert_eq!(transform.scale, [1.0, 1.0, 1.0]);
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```powershell
cargo test scale_classifier_accepts_taiwan_epsg_3826_meter_bounds scale_classifier_detects_centimeter_like_coordinates scale_classifier_detects_millimeter_like_coordinates scale_classifier_uses_percentile_bounds_when_raw_bbox_has_stray_points scale_classifier_quarantines_far_away_model source_transform_declares_canonical_space
```

Expected: fail because `georef` module does not exist.

- [ ] **Step 3: Implement scale classifier and canonical transform model**

Create `src/georef.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds2 {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Bounds2 {
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self { min_x, min_y, max_x, max_y }
    }

    pub fn scaled(self, scale: f64) -> Self {
        Self {
            min_x: self.min_x * scale,
            min_y: self.min_y * scale,
            max_x: self.max_x * scale,
            max_y: self.max_y * scale,
        }
    }

    pub fn width(self) -> f64 {
        (self.max_x - self.min_x).abs()
    }

    pub fn height(self) -> f64 {
        (self.max_y - self.min_y).abs()
    }

    pub fn center(self) -> [f64; 2] {
        [(self.min_x + self.max_x) * 0.5, (self.min_y + self.max_y) * 0.5]
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundsSummary {
    pub raw_bounds: Bounds2,
    pub percentile_bounds: Bounds2,
    pub centroid: [f64; 2],
}

impl BoundsSummary {
    pub fn from_raw_and_percentile(raw_bounds: Bounds2, percentile_bounds: Bounds2) -> Self {
        Self {
            raw_bounds,
            percentile_bounds,
            centroid: percentile_bounds.center(),
        }
    }

    pub fn scaled(self, scale: f64) -> Self {
        Self {
            raw_bounds: self.raw_bounds.scaled(scale),
            percentile_bounds: self.percentile_bounds.scaled(scale),
            centroid: [self.centroid[0] * scale, self.centroid[1] * scale],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aoi {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Aoi {
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self { min_x, min_y, max_x, max_y }
    }

    pub fn contains_bounds(self, bounds: Bounds2) -> bool {
        bounds.min_x >= self.min_x
            && bounds.max_x <= self.max_x
            && bounds.min_y >= self.min_y
            && bounds.max_y <= self.max_y
    }

    pub fn contains_point(self, point: [f64; 2]) -> bool {
        point[0] >= self.min_x
            && point[0] <= self.max_x
            && point[1] >= self.min_y
            && point[1] <= self.max_y
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScaleClassification {
    pub selected_scale: Option<f64>,
    pub status: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceTransform {
    pub source_crs: String,
    pub canonical_crs: String,
    pub translation: [f64; 3],
    pub rotation_quat_xyzw: [f64; 4],
    pub scale: [f64; 3],
    pub unit_scale_to_meter: f64,
    pub canonical_space: String,
}

impl SourceTransform {
    pub fn identity(source_crs: &str, unit_scale_to_meter: f64) -> Self {
        Self {
            source_crs: source_crs.to_string(),
            canonical_crs: "EPSG:3826".to_string(),
            translation: [0.0, 0.0, 0.0],
            rotation_quat_xyzw: [0.0, 0.0, 0.0, 1.0],
            scale: [unit_scale_to_meter, unit_scale_to_meter, unit_scale_to_meter],
            unit_scale_to_meter,
            canonical_space: "EPSG:3826 meters / local ENU / Z-up".to_string(),
        }
    }
}

pub fn classify_source_scale(
    bounds: &BoundsSummary,
    aoi: &Aoi,
    allowed_scales: &[f64],
) -> ScaleClassification {
    for scale in allowed_scales {
        let scaled = bounds.scaled(*scale);
        if aoi.contains_point(scaled.centroid)
            && aoi.contains_bounds(scaled.percentile_bounds)
            && scaled.percentile_bounds.width() > 0.01
            && scaled.percentile_bounds.height() > 0.01
        {
            let mut warnings = vec![];
            if !aoi.contains_bounds(scaled.raw_bounds) {
                warnings.push("raw bbox outside AOI; accepted by centroid and percentile bounds".to_string());
            }
            return ScaleClassification {
                selected_scale: Some(*scale),
                status: "inside_aoi".to_string(),
                warnings,
            };
        }
    }

    ScaleClassification {
        selected_scale: None,
        status: "outside_aoi".to_string(),
        warnings: vec!["source bounds outside AOI for all allowed scales".to_string()],
    }
}
```

Modify `src/lib.rs`:

```rust
pub mod georef;
```

- [ ] **Step 4: Run tests**

Run:

```powershell
cargo test scale_classifier_accepts_taiwan_epsg_3826_meter_bounds scale_classifier_detects_centimeter_like_coordinates scale_classifier_detects_millimeter_like_coordinates scale_classifier_uses_percentile_bounds_when_raw_bbox_has_stray_points scale_classifier_quarantines_far_away_model source_transform_declares_canonical_space
```

Expected: all pass.

- [ ] **Step 5: Commit**

```powershell
git add src/georef.rs src/lib.rs tests/project_pipeline.rs
git commit -m "Add georef scale classification"
```

---

### Task 4: IFC Inspect Metrics And Group Candidate Dump

**Files:**
- Modify: `src/inspect.rs`
- Create: `src/grouping.rs`
- Modify: `src/lib.rs`
- Test: `tests/project_pipeline.rs`

- [ ] **Step 1: Write failing test for IFC group summary**

Add to `tests/project_pipeline.rs`:

```rust
use ifc_to_3dtiles::grouping::summarize_ifc_groups;
use ifc_to_3dtiles::step::StepIndex;

#[test]
fn ifc_group_summary_counts_assigned_objects() {
    let ifc = "\
#10=IFCGROUP('g1',$,'Cable Group','',$);
#20=IFCRELASSIGNSTOGROUP('r1',$,'','',(#100,#101),$,#10);
#100=IFCBUILDINGELEMENTPROXY('a',$,'A','', $, $, $, $, $);
#101=IFCBUILDINGELEMENTPROXY('b',$,'B','', $, $, $, $, $);
";
    let index = StepIndex::parse(ifc);
    let groups = summarize_ifc_groups(&index);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].name, "Cable Group");
    assert_eq!(groups[0].object_count, 2);
}
```

- [ ] **Step 2: Run test and verify it fails**

Run:

```powershell
cargo test ifc_group_summary_counts_assigned_objects
```

Expected: fail because `grouping` module does not exist.

- [ ] **Step 3: Implement IFC group summary**

Create `src/grouping.rs`:

```rust
use crate::step::{StepIndex, decode_ifc_string, extract_first_ref, extract_refs};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupSummary {
    pub ifc_step_id: u32,
    pub name: String,
    pub object_count: usize,
}

pub fn summarize_ifc_groups(index: &StepIndex) -> Vec<GroupSummary> {
    let names: HashMap<u32, String> = index
        .entities_by_type("IFCGROUP")
        .filter_map(|entity| {
            let name = entity.args.get(2).map(|arg| decode_ifc_string(arg))?;
            Some((entity.id, name))
        })
        .collect();

    let mut counts = HashMap::<u32, usize>::new();
    for rel in index.entities_by_type("IFCRELASSIGNSTOGROUP") {
        let Some(group_id) = rel.args.get(6).and_then(|arg| extract_first_ref(arg)) else {
            continue;
        };
        let related = rel.args.get(4).map(|arg| extract_refs(arg)).unwrap_or_default();
        *counts.entry(group_id).or_default() += related.len();
    }

    let mut groups: Vec<GroupSummary> = names
        .into_iter()
        .map(|(ifc_step_id, name)| GroupSummary {
            ifc_step_id,
            name,
            object_count: counts.get(&ifc_step_id).copied().unwrap_or(0),
        })
        .collect();
    groups.sort_by(|a, b| b.object_count.cmp(&a.object_count).then_with(|| a.name.cmp(&b.name)));
    groups
}
```

Modify `src/lib.rs`:

```rust
pub mod grouping;
```

- [ ] **Step 4: Add inspect output `group_candidates.json` for IFC**

In `src/main.rs` inspect branch, after writing `source_manifest.json`, scan discovered IFC sources:

```rust
let mut all_groups = Vec::new();
for source in &manifest.sources {
    if source.format != ifc_to_3dtiles::project::SourceFormat::Ifc {
        continue;
    }
    let content = fs::read_to_string(&source.path)
        .with_context(|| format!("讀取 IFC 失敗：{}", source.path.display()))?;
    let index = ifc_to_3dtiles::step::StepIndex::parse(content);
    for group in ifc_to_3dtiles::grouping::summarize_ifc_groups(&index) {
        all_groups.push(serde_json::json!({
            "source_id": source.id,
            "kind": "ifc_group",
            "name": group.name,
            "object_count": group.object_count,
            "ifc_step_id": group.ifc_step_id
        }));
    }
}
let group_path = output.join("group_candidates.json");
fs::write(&group_path, serde_json::to_vec_pretty(&all_groups)?)
    .with_context(|| format!("寫入 group candidates 失敗：{}", group_path.display()))?;
println!("{}", group_path.display());
```

- [ ] **Step 5: Run test and sample inspect**

Run:

```powershell
cargo test ifc_group_summary_counts_assigned_objects
cargo run -- inspect `
  --input "C:\Users\stw_s\Desktop\ifc_to_3dtiles\sample_files\淡江大橋移交模型" `
  --output "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\inspect_tamkang" `
  --source-epsg 3826
```

Expected:

- Test passes.
- `group_candidates.json` exists.
- For current sample IFC, group candidates include `ItemSet-1` and `ItemSet-2`, confirming IFC group data is too coarse for final explode grouping.

- [ ] **Step 6: Commit**

```powershell
git add src/grouping.rs src/lib.rs src/main.rs tests/project_pipeline.rs
git commit -m "Add IFC group candidate inspect output"
```

---

### Task 4A: DGN/DWG Inspect Metadata Hierarchy Dump

**Files:**
- Create: `src/cad_metadata.rs`
- Modify: `src/lib.rs`
- Modify: `src/inspect.rs`
- Test: `tests/project_pipeline.rs`

This is the highest-priority DGN/DWG task. Even when geometry conversion is not yet reliable, inspect must preserve the hierarchy that later drives isolate, explode, search, and debug workflows.

- [ ] **Step 1: Write failing test for CAD hierarchy dump schema**

Add to `tests/project_pipeline.rs`:

```rust
use ifc_to_3dtiles::cad_metadata::{
    CadHierarchyDump, CadModel, CadLevel, CadMaterial, CadReference,
};

#[test]
fn cad_hierarchy_dump_preserves_dgn_metadata_buckets() {
    let dump = CadHierarchyDump {
        source_id: "bridge-dgn".to_string(),
        models: vec![CadModel { name: "Default".to_string(), element_count: 120 }],
        references: vec![CadReference { name: "pier-ref".to_string(), path: "pier.dgn".to_string() }],
        levels: vec![CadLevel { name: "Cable".to_string(), element_count: 80 }],
        cells: vec![],
        shared_cells: vec![],
        attachments: vec![],
        element_classes: vec!["Primary".to_string()],
        materials: vec![CadMaterial { name: "concrete".to_string(), color_rgba: [0.8, 0.8, 0.78, 1.0] }],
        line_styles: vec!["ByLevel".to_string()],
    };

    let json = serde_json::to_value(&dump).expect("serialize CAD dump");
    assert!(json.get("models").is_some());
    assert!(json.get("references").is_some());
    assert!(json.get("levels").is_some());
    assert!(json.get("cells").is_some());
    assert!(json.get("shared_cells").is_some());
    assert!(json.get("attachments").is_some());
    assert!(json.get("element_classes").is_some());
    assert!(json.get("materials").is_some());
    assert!(json.get("line_styles").is_some());
}
```

- [ ] **Step 2: Run test and verify it fails**

Run:

```powershell
cargo test cad_hierarchy_dump_preserves_dgn_metadata_buckets
```

Expected: fail because `cad_metadata` module does not exist.

- [ ] **Step 3: Implement schema-first CAD dump module**

Create `src/cad_metadata.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadHierarchyDump {
    pub source_id: String,
    pub models: Vec<CadModel>,
    pub references: Vec<CadReference>,
    pub levels: Vec<CadLevel>,
    pub cells: Vec<CadCell>,
    pub shared_cells: Vec<CadCell>,
    pub attachments: Vec<CadAttachment>,
    pub element_classes: Vec<String>,
    pub materials: Vec<CadMaterial>,
    pub line_styles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadModel {
    pub name: String,
    pub element_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadReference {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadLevel {
    pub name: String,
    pub element_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadCell {
    pub name: String,
    pub element_count: usize,
    pub center: Option<[f64; 3]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadAttachment {
    pub name: String,
    pub path: String,
    pub transform_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadMaterial {
    pub name: String,
    pub color_rgba: [f32; 4],
}
```

Modify `src/lib.rs`:

```rust
pub mod cad_metadata;
```

- [ ] **Step 4: Add inspect output contract**

When the inspect route sees `.dgn` or `.dwg`, write a sidecar dump path in the source manifest:

```text
out/inspect_tamkang/cad_metadata/<source-id>.json
```

If no open inspection tool is available, still write an empty bucket schema and a warning such as:

```text
CAD hierarchy probe unavailable; geometry conversion may continue, but reference/model/level/cell metadata is incomplete.
```

- [ ] **Step 5: Commit**

```powershell
git add src/cad_metadata.rs src/lib.rs src/inspect.rs tests/project_pipeline.rs
git commit -m "Add CAD hierarchy inspect metadata schema"
```

---

### Task 5: Quarantine Rules And Source Approval

**Files:**
- Modify: `src/project.rs`
- Modify: `src/georef.rs`
- Modify: `src/main.rs`
- Test: `tests/project_pipeline.rs`

- [ ] **Step 1: Write failing tests for quarantine decisions**

Add to `tests/project_pipeline.rs`:

```rust
use ifc_to_3dtiles::georef::{Aoi, Bounds2, BoundsSummary, decide_source_status};
use ifc_to_3dtiles::project::SourceStatus;

#[test]
fn decide_source_status_approves_inside_aoi_3d_source() {
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let status = decide_source_status(
        BoundsSummary::from_raw_and_percentile(
            Bounds2::new(300_000.0, 2_787_000.0, 301_000.0, 2_788_000.0),
            Bounds2::new(300_000.0, 2_787_000.0, 301_000.0, 2_788_000.0),
        ),
        20.0,
        &aoi,
        &[1000.0, 1.0, 0.1, 0.01, 0.001],
    );
    assert_eq!(status.status, SourceStatus::Approved);
    assert_eq!(status.selected_scale, Some(1.0));
}

#[test]
fn decide_source_status_quarantines_flat_2d_source() {
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let status = decide_source_status(
        BoundsSummary::from_raw_and_percentile(
            Bounds2::new(300_000.0, 2_787_000.0, 301_000.0, 2_788_000.0),
            Bounds2::new(300_000.0, 2_787_000.0, 301_000.0, 2_788_000.0),
        ),
        0.001,
        &aoi,
        &[1000.0, 1.0, 0.1, 0.01, 0.001],
    );
    assert_eq!(status.status, SourceStatus::Quarantined);
    assert!(status.warnings.iter().any(|w| w.contains("2D")));
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```powershell
cargo test decide_source_status_approves_inside_aoi_3d_source decide_source_status_quarantines_flat_2d_source
```

Expected: fail because `decide_source_status` does not exist.

- [ ] **Step 3: Implement status decision**

Add to `src/georef.rs`:

```rust
use crate::project::SourceStatus;

#[derive(Debug, Clone, PartialEq)]
pub struct SourceStatusDecision {
    pub status: SourceStatus,
    pub selected_scale: Option<f64>,
    pub warnings: Vec<String>,
}

pub fn decide_source_status(
    bounds_xy: BoundsSummary,
    z_range_m: f64,
    aoi: &Aoi,
    allowed_scales: &[f64],
) -> SourceStatusDecision {
    let scale = classify_source_scale(&bounds_xy, aoi, allowed_scales);
    let mut warnings = scale.warnings;

    if z_range_m.abs() < 0.05 {
        warnings.push("source appears 2D because z range is below 5cm".to_string());
        return SourceStatusDecision {
            status: SourceStatus::Quarantined,
            selected_scale: scale.selected_scale,
            warnings,
        };
    }

    if scale.selected_scale.is_none() {
        return SourceStatusDecision {
            status: SourceStatus::Quarantined,
            selected_scale: None,
            warnings,
        };
    }

    SourceStatusDecision {
        status: SourceStatus::Approved,
        selected_scale: scale.selected_scale,
        warnings,
    }
}
```

- [ ] **Step 4: Run tests**

Run:

```powershell
cargo test decide_source_status_approves_inside_aoi_3d_source decide_source_status_quarantines_flat_2d_source
```

Expected: pass.

- [ ] **Step 5: Commit**

```powershell
git add src/georef.rs tests/project_pipeline.rs
git commit -m "Add source quarantine decision rules"
```

---

### Task 6: CAD Probe Report Integration

**Files:**
- Modify: `tools/inspect_cad_sources.ps1`
- Modify: `src/inspect.rs`
- Modify: `docs/local_project_workflow.md`
- Test: `tests/project_pipeline.rs`

- [ ] **Step 1: Write failing test for importing CAD probe summary**

Add to `tests/project_pipeline.rs`:

```rust
use ifc_to_3dtiles::inspect::CadProbeSummary;

#[test]
fn cad_probe_summary_marks_old_oda_converter() {
    let json = r#"{
      "tools": {
        "ogrinfo": {"found": true, "source": "C:\\ms4w_MSSQL\\GDAL\\ogrinfo.exe"},
        "ogr2ogr": {"found": true, "source": "C:\\ms4w_MSSQL\\GDAL\\ogr2ogr.exe"},
        "oda_file_converter": {
          "found": true,
          "source": "C:\\bin\\ODAFileConverter\\ODAFileConverter.exe",
          "version": "20.12.0.0",
          "version_risk": "too_old_for_2026_cad_delivery"
        }
      },
      "file_count": 8,
      "cad_file_count": 7,
      "extension_distribution": [
        {"extension": ".dwg", "count": 4, "total_bytes": 149951738},
        {"extension": ".dgn", "count": 3, "total_bytes": 240211456},
        {"extension": ".ifc", "count": 1, "total_bytes": 117439099}
      ]
    }"#;

    let summary: CadProbeSummary = serde_json::from_str(json).expect("parse probe summary");
    assert_eq!(summary.cad_file_count, 7);
    assert_eq!(summary.tools.oda_file_converter.version_risk.as_deref(), Some("too_old_for_2026_cad_delivery"));
}
```

- [ ] **Step 2: Run test and verify it fails**

Run:

```powershell
cargo test cad_probe_summary_marks_old_oda_converter
```

Expected: fail because `CadProbeSummary` does not exist.

- [ ] **Step 3: Implement probe summary structs**

Add to `src/inspect.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadProbeSummary {
    pub tools: CadProbeTools,
    pub file_count: usize,
    pub cad_file_count: usize,
    pub extension_distribution: Vec<CadExtensionSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadProbeTools {
    pub ogrinfo: CadToolSummary,
    pub ogr2ogr: CadToolSummary,
    pub oda_file_converter: CadToolSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadToolSummary {
    pub found: bool,
    pub source: Option<String>,
    pub version: Option<String>,
    pub version_risk: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadExtensionSummary {
    pub extension: String,
    pub count: usize,
    pub total_bytes: f64,
}
```

- [ ] **Step 4: Run CAD probe and test**

Run:

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\inspect_cad_sources.ps1
cargo test cad_probe_summary_marks_old_oda_converter
```

Expected:

- `out\cad_probe\cad_probe_report.json` exists.
- Missing GDAL/ODA tools are recorded as `found=false` instead of failing.
- ODA File Converter `20.12.0.0` is marked `too_old_for_2026_cad_delivery`.
- Sample extension distribution is visible before conversion.

- [ ] **Step 5: Commit**

```powershell
git add tools/inspect_cad_sources.ps1 src/inspect.rs tests/project_pipeline.rs docs/local_project_workflow.md
git commit -m "Integrate CAD probe report into inspect workflow"
```

---

### Task 6A: Geometry Fingerprint And Duplicate Detection

**Files:**
- Create: `src/fingerprint.rs`
- Modify: `src/lib.rs`
- Modify: `src/inspect.rs`
- Test: `tests/project_pipeline.rs`

- [ ] **Step 1: Write failing tests for duplicate fingerprint candidates**

Add to `tests/project_pipeline.rs`:

```rust
use ifc_to_3dtiles::fingerprint::{GeometryFingerprint, duplicate_candidate_score};

#[test]
fn geometry_fingerprint_detects_probable_duplicate_sources() {
    let a = GeometryFingerprint {
        source_id: "bridge-ifc".to_string(),
        vertex_count: 100_000,
        triangle_count: 180_000,
        bbox: [300_000.0, 2_787_000.0, 0.0, 301_000.0, 2_788_000.0, 60.0],
        surface_area_m2: 125_000.0,
        hash: "a".to_string(),
    };
    let b = GeometryFingerprint {
        source_id: "bridge-dgn".to_string(),
        vertex_count: 100_250,
        triangle_count: 179_900,
        bbox: [300_000.2, 2_787_000.1, 0.0, 301_000.1, 2_788_000.2, 60.1],
        surface_area_m2: 125_100.0,
        hash: "b".to_string(),
    };

    let score = duplicate_candidate_score(&a, &b);
    assert!(score >= 0.95);
}
```

- [ ] **Step 2: Run test and verify it fails**

Run:

```powershell
cargo test geometry_fingerprint_detects_probable_duplicate_sources
```

Expected: fail because `fingerprint` module does not exist.

- [ ] **Step 3: Implement fingerprint summary**

Create `src/fingerprint.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeometryFingerprint {
    pub source_id: String,
    pub vertex_count: u64,
    pub triangle_count: u64,
    pub bbox: [f64; 6],
    pub surface_area_m2: f64,
    pub hash: String,
}

pub fn duplicate_candidate_score(a: &GeometryFingerprint, b: &GeometryFingerprint) -> f64 {
    let tri_ratio = ratio_score(a.triangle_count as f64, b.triangle_count as f64);
    let vertex_ratio = ratio_score(a.vertex_count as f64, b.vertex_count as f64);
    let area_ratio = ratio_score(a.surface_area_m2, b.surface_area_m2);
    let bbox_ratio = bbox_similarity(a.bbox, b.bbox);
    (tri_ratio + vertex_ratio + area_ratio + bbox_ratio) / 4.0
}

fn ratio_score(a: f64, b: f64) -> f64 {
    if a <= 0.0 || b <= 0.0 {
        return 0.0;
    }
    a.min(b) / a.max(b)
}

fn bbox_similarity(a: [f64; 6], b: [f64; 6]) -> f64 {
    let mut total = 0.0;
    for i in 0..6 {
        let scale = a[i].abs().max(b[i].abs()).max(1.0);
        total += 1.0 - ((a[i] - b[i]).abs() / scale).min(1.0);
    }
    total / 6.0
}
```

Modify `src/lib.rs`:

```rust
pub mod fingerprint;
```

- [ ] **Step 4: Inspect output contract**

Add `fingerprint` and `duplicate_candidates` to inspect output. A source must be quarantined or require explicit review when:

- The score is `>= 0.95` against an approved source.
- The source has nearly identical bbox and triangle/vertex counts to another source but different format.
- The source overlaps a published source strongly enough to risk duplicate bridge geometry.

- [ ] **Step 5: Commit**

```powershell
git add src/fingerprint.rs src/lib.rs src/inspect.rs tests/project_pipeline.rs
git commit -m "Add geometry duplicate fingerprinting"
```

---

### Task 7: Publish Root Tilesets For Approved Per-Source Tilesets

**Files:**
- Create: `src/publish.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs`
- Test: `tests/project_pipeline.rs`

- [ ] **Step 1: Write failing tests for publish root tileset list**

Add to `tests/project_pipeline.rs`:

```rust
use ifc_to_3dtiles::publish::{
    GroupManifestEntry, PublishSource, SourceManifestEntry, build_publish_tileset,
};

#[test]
fn publish_tileset_wraps_child_tilesets_with_source_metadata() {
    let sources = vec![
        PublishSource {
            source_id: "main-ifc".to_string(),
            label: "DJB-M-SU-監測.ifc".to_string(),
            tileset_uri: "sources/main-ifc/tileset.json".to_string(),
            normal_mode: "flat".to_string(),
            bbox: [300_000.0, 2_787_000.0, 0.0, 301_000.0, 2_788_000.0, 60.0],
        },
        PublishSource {
            source_id: "main-ifc".to_string(),
            label: "DJB-M-SU-監測.ifc".to_string(),
            tileset_uri: "sources/main-ifc/tileset_smooth_90.json".to_string(),
            normal_mode: "smooth_90".to_string(),
            bbox: [300_000.0, 2_787_000.0, 0.0, 301_000.0, 2_788_000.0, 60.0],
        },
    ];
    let tileset = build_publish_tileset(&sources, "flat");
    assert_eq!(tileset["asset"]["version"], "1.0");
    assert_eq!(tileset["root"]["children"].as_array().unwrap().len(), 1);
    assert_eq!(tileset["root"]["children"][0]["extras"]["source_id"], "main-ifc");
}

#[test]
fn publish_manifests_include_source_and_group_debug_contract() {
    let source = SourceManifestEntry {
        source_id: "main-dgn".to_string(),
        format: "dgn".to_string(),
        status: "approved".to_string(),
        transform: serde_json::json!({
            "translation": [0.0, 0.0, 0.0],
            "rotation": [0.0, 0.0, 0.0, 1.0],
            "scale": [1.0, 1.0, 1.0]
        }),
        bbox: serde_json::json!({
            "raw": [300000.0, 2787000.0, 0.0, 301000.0, 2788000.0, 60.0],
            "percentile": [300010.0, 2787010.0, 0.0, 300990.0, 2787990.0, 60.0]
        }),
        group_stats: serde_json::json!({"level:Cable": 80}),
        materials: serde_json::json!({"concrete": {"color_rgba": [0.8, 0.8, 0.78, 1.0]}}),
        warnings: vec![],
    };
    let group = GroupManifestEntry {
        group_key: "level:Cable".to_string(),
        label: "Cable".to_string(),
        source_ids: vec!["main-dgn".to_string()],
        feature_count: 80,
        bbox: [300_000.0, 2_787_000.0, 0.0, 301_000.0, 2_788_000.0, 60.0],
        center: [300_500.0, 2_787_500.0, 30.0],
        explode_origin: [300_500.0, 2_787_500.0, 30.0],
        material_summary: serde_json::json!({"concrete": 80}),
    };

    assert_eq!(source.transform["scale"][0], 1.0);
    assert_eq!(group.explode_origin, group.center);
}
```

This test is intentionally validating root wrapping only. It must not combine child geometry buffers or rewrite child `.b3dm` payloads.

- [ ] **Step 2: Run test and verify it fails**

Run:

```powershell
cargo test publish_tileset_wraps_child_tilesets_with_source_metadata
```

Expected: fail because `publish` module does not exist.

- [ ] **Step 3: Implement publish tileset builder**

Create `src/publish.rs`:

```rust
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishSource {
    pub source_id: String,
    pub label: String,
    pub tileset_uri: String,
    pub normal_mode: String,
    pub bbox: [f64; 6],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceManifestEntry {
    pub source_id: String,
    pub format: String,
    pub status: String,
    pub transform: Value,
    pub bbox: Value,
    pub group_stats: Value,
    pub materials: Value,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupManifestEntry {
    pub group_key: String,
    pub label: String,
    pub source_ids: Vec<String>,
    pub feature_count: usize,
    pub bbox: [f64; 6],
    pub center: [f64; 3],
    pub explode_origin: [f64; 3],
    pub material_summary: Value,
}

pub fn build_publish_tileset(sources: &[PublishSource], normal_mode: &str) -> Value {
    let children: Vec<Value> = sources
        .iter()
        .filter(|source| source.normal_mode == normal_mode)
        .map(|source| {
            json!({
                "boundingVolume": {
                    "box": [0.0, 0.0, 0.0, 1000000.0, 0.0, 0.0, 0.0, 1000000.0, 0.0, 0.0, 0.0, 1000000.0]
                },
                "geometricError": 0,
                "refine": "ADD",
                "content": { "uri": source.tileset_uri },
                "extras": {
                    "source_id": source.source_id,
                    "label": source.label,
                    "bbox": source.bbox,
                    "normal_mode": source.normal_mode
                }
            })
        })
        .collect();

    json!({
        "asset": {
            "version": "1.0",
            "generator": "ifc_to_3dtiles publish"
        },
        "geometricError": 1000,
        "root": {
            "boundingVolume": {
                "box": [0.0, 0.0, 0.0, 1000000.0, 0.0, 0.0, 0.0, 1000000.0, 0.0, 0.0, 0.0, 1000000.0]
            },
            "geometricError": 1000,
            "refine": "ADD",
            "children": children
        }
    })
}
```

Modify `src/lib.rs`:

```rust
pub mod publish;
```

Also add helper functions `build_source_manifest_entries(project: &ProjectManifest)` and `build_group_manifest_entries(project: &ProjectManifest)`. These should map approved `SourceRecord` values into the publish manifest schema and aggregate group records from `explode_group_key` plus inspect summaries. If group centers cannot yet be computed from geometry, write `null` / warning instead of fabricating a center.

- [ ] **Step 4: Add CLI publish skeleton**

Add `Publish` subcommand:

```rust
Publish {
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long)]
    output: PathBuf,
}
```

Implementation:

```rust
if let Some(Command::Publish { manifest, output }) = cli.command {
    fs::create_dir_all(&output)
        .with_context(|| format!("建立 publish 輸出目錄失敗：{}", output.display()))?;
    let manifest_text = fs::read_to_string(&manifest)
        .with_context(|| format!("讀取 manifest 失敗：{}", manifest.display()))?;
    let project: ifc_to_3dtiles::project::ProjectManifest = serde_json::from_str(&manifest_text)?;
    let sources: Vec<ifc_to_3dtiles::publish::PublishSource> = project.sources
        .iter()
        .filter(|source| source.status == ifc_to_3dtiles::project::SourceStatus::Approved)
        .flat_map(|source| {
            [
                ("flat", "tileset.json"),
                ("smooth_90", "tileset_smooth_90.json"),
                ("smooth", "tileset_smooth.json"),
            ]
            .into_iter()
            .map(move |(normal_mode, name)| ifc_to_3dtiles::publish::PublishSource {
                source_id: source.id.clone(),
                label: source.path.file_name().and_then(|s| s.to_str()).unwrap_or(&source.id).to_string(),
                tileset_uri: format!("sources/{}/{}", source.id, name),
                normal_mode: normal_mode.to_string(),
                bbox: source.raw_bbox.unwrap_or([0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            })
        })
        .collect();
    fs::write(output.join("tileset.json"), serde_json::to_vec_pretty(&ifc_to_3dtiles::publish::build_publish_tileset(&sources, "flat"))?)?;
    fs::write(output.join("tileset_smooth_90.json"), serde_json::to_vec_pretty(&ifc_to_3dtiles::publish::build_publish_tileset(&sources, "smooth_90"))?)?;
    fs::write(output.join("tileset_smooth.json"), serde_json::to_vec_pretty(&ifc_to_3dtiles::publish::build_publish_tileset(&sources, "smooth"))?)?;
    fs::write(output.join("sources_manifest.json"), serde_json::to_vec_pretty(&build_source_manifest_entries(&project))?)?;
    fs::write(output.join("groups.json"), serde_json::to_vec_pretty(&build_group_manifest_entries(&project))?)?;
    println!("{}", output.display());
    return Ok(());
}
```

The publish layer must also write these debug manifests:

```json
{
  "source_id": "main-dgn",
  "format": "dgn",
  "status": "approved",
  "transform": {
    "translation": [0.0, 0.0, 0.0],
    "rotation": [0.0, 0.0, 0.0, 1.0],
    "scale": [1.0, 1.0, 1.0]
  },
  "bbox": {
    "raw": [300000.0, 2787000.0, 0.0, 301000.0, 2788000.0, 60.0],
    "percentile": [300010.0, 2787010.0, 0.0, 300990.0, 2787990.0, 60.0]
  },
  "group_stats": {
    "level:Cable": 80
  },
  "materials": {
    "concrete": {
      "color_rgba": [0.8, 0.8, 0.78, 1.0]
    }
  },
  "warnings": []
}
```

`groups.json` must keep the spatial center needed for shader explode:

```json
{
  "group_key": "level:Cable",
  "label": "Cable",
  "source_ids": ["main-dgn"],
  "feature_count": 80,
  "bbox": [300000.0, 2787000.0, 0.0, 301000.0, 2788000.0, 60.0],
  "center": [300500.0, 2787500.0, 30.0],
  "explode_origin": [300500.0, 2787500.0, 30.0],
  "material_summary": {
    "concrete": 80
  }
}
```

- [ ] **Step 5: Run tests**

Run:

```powershell
cargo test publish_tileset_wraps_child_tilesets_with_source_metadata
```

Expected: pass.

- [ ] **Step 6: Commit**

```powershell
git add src/publish.rs src/lib.rs src/main.rs tests/project_pipeline.rs
git commit -m "Add publish root tileset builder"
```

---

### Task 8: Metadata Extension For Source And Explode Group

**Files:**
- Modify: `src/convert.rs`
- Test: `tests/core.rs`

Feature metadata keeps the lookup key; `groups.json` keeps group-level spatial data such as `center` and `explode_origin`. Do not duplicate large group tables into every feature. For DGN/DWG-derived sources, candidate group priority is `reference -> model -> level -> cell/shared_cell -> material -> element_class`, because IFC group data may be absent or too coarse.

- [ ] **Step 1: Write failing test for Batch Table source fields**

Add to `tests/core.rs`:

```rust
#[test]
fn batch_table_includes_source_and_explode_group_fields() {
    let metadata = ifc_to_3dtiles::convert::FeatureMetadata {
        batch_id: 0,
        ifc_step_id: 1,
        global_id: "g".to_string(),
        ifc_type: "IFCBUILDINGELEMENTPROXY".to_string(),
        name: "n".to_string(),
        description: "0 , model.dgn, Default:5317".to_string(),
        dgn_element: "5317".to_string(),
        site: "Site".to_string(),
        building: "Bldg".to_string(),
        storey: "Level 02".to_string(),
        group_names: vec!["Cable".to_string()],
        style_id: "#1".to_string(),
        color_rgba: [1.0, 1.0, 1.0, 1.0],
        psets_json: "{}".to_string(),
        source_file: "model.dgn".to_string(),
        source_id: "model-dgn".to_string(),
        explode_group_key: "group:Cable".to_string(),
    };
    let table = ifc_to_3dtiles::convert::feature_metadata_batch_table(&[metadata]);
    assert_eq!(table["source_file"][0], "model.dgn");
    assert_eq!(table["source_id"][0], "model-dgn");
    assert_eq!(table["explode_group_key"][0], "group:Cable");
}
```

- [ ] **Step 2: Run test and verify it fails**

Run:

```powershell
cargo test batch_table_includes_source_and_explode_group_fields
```

Expected: fail because fields and helper are not public yet.

- [ ] **Step 3: Add metadata fields and public helper**

Modify `FeatureMetadata` in `src/convert.rs`:

```rust
pub source_file: String,
pub source_id: String,
pub explode_group_key: String,
```

When building metadata, set:

```rust
let source_file = parse_source_file_from_description(&description);
let source_id = source_file
    .chars()
    .map(|ch| if ch.is_ascii_alphanumeric() { ch.to_ascii_lowercase() } else { '-' })
    .collect::<String>();
let explode_group_key = if let Some(first_group) = ctx.group_names_by_object.get(&entity.id).and_then(|groups| groups.first()) {
    format!("group:{first_group}")
} else if !storey.is_empty() {
    format!("storey:{storey}")
} else if !dgn_element.is_empty() {
    format!("element:{dgn_element}")
} else {
    format!("ifc_type:{}", entity.type_name)
};
```

For CAD-derived sources, resolve `explode_group_key` from inspect metadata in this order:

```rust
let explode_group_key = cad_reference
    .map(|name| format!("reference:{name}"))
    .or_else(|| cad_model.map(|name| format!("model:{name}")))
    .or_else(|| cad_level.map(|name| format!("level:{name}")))
    .or_else(|| cad_cell.map(|name| format!("cell:{name}")))
    .or_else(|| cad_material.map(|name| format!("material:{name}")))
    .unwrap_or_else(|| format!("source:{source_id}"));
```

Extract current private batch table builder into:

```rust
pub fn feature_metadata_batch_table(metadata: &[FeatureMetadata]) -> Value {
    let mut object = Map::new();
    object.insert("batch_id".to_string(), Value::Array(metadata.iter().map(|m| json!(m.batch_id)).collect()));
    object.insert("ifc_step_id".to_string(), Value::Array(metadata.iter().map(|m| json!(m.ifc_step_id)).collect()));
    object.insert("global_id".to_string(), Value::Array(metadata.iter().map(|m| json!(m.global_id)).collect()));
    object.insert("ifc_type".to_string(), Value::Array(metadata.iter().map(|m| json!(m.ifc_type)).collect()));
    object.insert("name".to_string(), Value::Array(metadata.iter().map(|m| json!(m.name)).collect()));
    object.insert("description".to_string(), Value::Array(metadata.iter().map(|m| json!(m.description)).collect()));
    object.insert("dgn_element".to_string(), Value::Array(metadata.iter().map(|m| json!(m.dgn_element)).collect()));
    object.insert("site".to_string(), Value::Array(metadata.iter().map(|m| json!(m.site)).collect()));
    object.insert("building".to_string(), Value::Array(metadata.iter().map(|m| json!(m.building)).collect()));
    object.insert("storey".to_string(), Value::Array(metadata.iter().map(|m| json!(m.storey)).collect()));
    object.insert("group_names".to_string(), Value::Array(metadata.iter().map(|m| json!(m.group_names)).collect()));
    object.insert("style_id".to_string(), Value::Array(metadata.iter().map(|m| json!(m.style_id)).collect()));
    object.insert("color_rgba".to_string(), Value::Array(metadata.iter().map(|m| json!(m.color_rgba)).collect()));
    object.insert("psets_json".to_string(), Value::Array(metadata.iter().map(|m| json!(m.psets_json)).collect()));
    object.insert("source_file".to_string(), Value::Array(metadata.iter().map(|m| json!(m.source_file)).collect()));
    object.insert("source_id".to_string(), Value::Array(metadata.iter().map(|m| json!(m.source_id)).collect()));
    object.insert("explode_group_key".to_string(), Value::Array(metadata.iter().map(|m| json!(m.explode_group_key)).collect()));
    Value::Object(object)
}
```

- [ ] **Step 4: Run tests**

Run:

```powershell
cargo test batch_table_includes_source_and_explode_group_fields
cargo test
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```powershell
git add src/convert.rs tests/core.rs
git commit -m "Add source and explode group metadata"
```

---

### Task 8B: SQLite Property Database

**Files:**
- Modify: `Cargo.toml`
- Create: `src/properties_db.rs`
- Modify: `src/lib.rs`
- Modify: `src/publish.rs`
- Test: `tests/project_pipeline.rs`

SQLite is the canonical property store. Batch Table metadata should keep lightweight lookup keys only; full source, group, material, transform, warning, duplicate, and property data belongs in `properties.sqlite`. Static viewers may receive a small derived JSON index, but the trusted source remains SQLite.

- [ ] **Step 1: Add SQLite dependency**

Modify `Cargo.toml`:

```toml
rusqlite = { version = "0.32", features = ["bundled"] }
```

- [ ] **Step 2: Write failing test for property DB schema**

Add to `tests/project_pipeline.rs`:

```rust
use ifc_to_3dtiles::properties_db::{
    FeaturePropertyRecord, SourcePropertyRecord, init_property_db, insert_feature_property,
    insert_source_property,
};

#[test]
fn property_db_preserves_source_and_feature_lookup_data() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("properties.sqlite");
    let conn = init_property_db(&db_path).expect("init db");

    insert_source_property(&conn, &SourcePropertyRecord {
        source_id: "main-dgn".to_string(),
        format: "dgn".to_string(),
        status: "approved".to_string(),
        crs: "EPSG:3826".to_string(),
        scale_candidate: 1.0,
        transform_json: r#"{"translation":[0,0,0],"rotation":[0,0,0,1],"scale":[1,1,1]}"#.to_string(),
        warnings_json: "[]".to_string(),
    }).expect("insert source");

    insert_feature_property(&conn, &FeaturePropertyRecord {
        source_id: "main-dgn".to_string(),
        feature_key: "main-dgn:42".to_string(),
        batch_id: 42,
        global_id: "".to_string(),
        ifc_step_id: None,
        ifc_type: "".to_string(),
        name: "Cable member".to_string(),
        explode_group_key: "level:Cable".to_string(),
        properties_json: r#"{"level":"Cable","material":"steel"}"#.to_string(),
    }).expect("insert feature");

    let count: i64 = conn
        .query_row("select count(*) from feature_properties where explode_group_key = 'level:Cable'", [], |row| row.get(0))
        .expect("query feature count");
    assert_eq!(count, 1);
}
```

- [ ] **Step 3: Run test and verify it fails**

Run:

```powershell
cargo test property_db_preserves_source_and_feature_lookup_data
```

Expected: fail because `properties_db` module does not exist.

- [ ] **Step 4: Implement schema and writers**

Create `src/properties_db.rs`:

```rust
use anyhow::Result;
use rusqlite::{Connection, params};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct SourcePropertyRecord {
    pub source_id: String,
    pub format: String,
    pub status: String,
    pub crs: String,
    pub scale_candidate: f64,
    pub transform_json: String,
    pub warnings_json: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FeaturePropertyRecord {
    pub source_id: String,
    pub feature_key: String,
    pub batch_id: i64,
    pub global_id: String,
    pub ifc_step_id: Option<i64>,
    pub ifc_type: String,
    pub name: String,
    pub explode_group_key: String,
    pub properties_json: String,
}

pub fn init_property_db(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        r#"
        pragma journal_mode = wal;
        create table if not exists source_properties (
            source_id text primary key,
            format text not null,
            status text not null,
            crs text not null,
            scale_candidate real not null,
            transform_json text not null,
            warnings_json text not null
        );
        create table if not exists feature_properties (
            feature_key text primary key,
            source_id text not null,
            batch_id integer not null,
            global_id text not null,
            ifc_step_id integer,
            ifc_type text not null,
            name text not null,
            explode_group_key text not null,
            properties_json text not null
        );
        create index if not exists idx_feature_source on feature_properties(source_id);
        create index if not exists idx_feature_group on feature_properties(explode_group_key);
        create index if not exists idx_feature_global_id on feature_properties(global_id);
        "#,
    )?;
    Ok(conn)
}

pub fn insert_source_property(conn: &Connection, source: &SourcePropertyRecord) -> Result<()> {
    conn.execute(
        r#"
        insert into source_properties (
            source_id, format, status, crs, scale_candidate, transform_json, warnings_json
        ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        on conflict(source_id) do update set
            format = excluded.format,
            status = excluded.status,
            crs = excluded.crs,
            scale_candidate = excluded.scale_candidate,
            transform_json = excluded.transform_json,
            warnings_json = excluded.warnings_json
        "#,
        params![
            source.source_id,
            source.format,
            source.status,
            source.crs,
            source.scale_candidate,
            source.transform_json,
            source.warnings_json
        ],
    )?;
    Ok(())
}

pub fn insert_feature_property(conn: &Connection, feature: &FeaturePropertyRecord) -> Result<()> {
    conn.execute(
        r#"
        insert into feature_properties (
            feature_key, source_id, batch_id, global_id, ifc_step_id, ifc_type,
            name, explode_group_key, properties_json
        ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        on conflict(feature_key) do update set
            source_id = excluded.source_id,
            batch_id = excluded.batch_id,
            global_id = excluded.global_id,
            ifc_step_id = excluded.ifc_step_id,
            ifc_type = excluded.ifc_type,
            name = excluded.name,
            explode_group_key = excluded.explode_group_key,
            properties_json = excluded.properties_json
        "#,
        params![
            feature.feature_key,
            feature.source_id,
            feature.batch_id,
            feature.global_id,
            feature.ifc_step_id,
            feature.ifc_type,
            feature.name,
            feature.explode_group_key,
            feature.properties_json
        ],
    )?;
    Ok(())
}
```

Modify `src/lib.rs`:

```rust
pub mod properties_db;
```

- [ ] **Step 5: Publish DB contract**

During publish, write:

```text
publish/
  properties.sqlite
  properties_lookup.json
```

Rules:

- `properties.sqlite` is canonical.
- `properties_lookup.json` is a small static-viewer index with `feature_key -> source_id / batch_id / explode_group_key / display_name`.
- Do not store large raw STEP or full CAD dumps inside Batch Table.
- Keep `feature_key` stable across flat / smooth 90 / smooth 180 normal tilesets.

- [ ] **Step 6: Run tests**

Run:

```powershell
cargo test property_db_preserves_source_and_feature_lookup_data
```

Expected: pass.

- [ ] **Step 7: Commit**

```powershell
git add Cargo.toml Cargo.lock src/properties_db.rs src/lib.rs src/publish.rs tests/project_pipeline.rs
git commit -m "Add SQLite property database"
```

---

### Task 8A: Viewer Debug Contract And Shader Explode

**Files:**
- Modify: `out/DJB-M-SU-_/index.html`
- Modify: `out/DJB-M-SU-_/index_three.html`
- Modify: `out/DJB-M-SU-_/index_maplibre_three.html`
- Test: `tools/verify_index_page.ps1`

- [ ] **Step 1: Add viewer verification checks**

Extend `tools/verify_index_page.ps1` to check that the production viewer and comparison viewers expose these controls or data hooks:

- Isolate source.
- Isolate group.
- Search property.
- Highlight selected feature.
- Hide/show source and group.
- Transparency by source and group.
- AOI clipping.
- Section plane.
- Flood simulation / water elevation test.
- Terrain offset.
- Debug source bbox.
- Debug source transform.
- Debug CRS.
- Debug scale candidate.
- Debug quarantine reason.

- [ ] **Step 2: Implement shader explode contract**

Explode must be a rendering effect, not a geometry rewrite. Use group spatial centers from `groups.json`:

```glsl
vec3 explodeDir = normalize(groupCenter - modelCenter);
vec3 explodeOffset = explodeDir * explodeAmount;
worldPosition.xyz += explodeOffset;
```

Rules:

- Do not rebuild `.b3dm` for explode.
- Do not duplicate selected geometry just to explode groups.
- Keep batching where the renderer allows custom shader/material hooks.
- Fallback to overlay mesh only when the renderer cannot inject a per-feature/group shader.

- [ ] **Step 3: Keep source and group debug visible**

Viewer debug panel must show:

```json
{
  "source_id": "main-dgn",
  "format": "dgn",
  "crs": "EPSG:3826",
  "scale_candidate": 1.0,
  "transform": {
    "translation": [0.0, 0.0, 0.0],
    "rotation": [0.0, 0.0, 0.0, 1.0],
    "scale": [1.0, 1.0, 1.0]
  },
  "bbox_mode": "centroid_percentile",
  "quarantine_reason": null
}
```

- [ ] **Step 4: Commit**

```powershell
git add out/DJB-M-SU-_/index.html out/DJB-M-SU-_/index_three.html out/DJB-M-SU-_/index_maplibre_three.html tools/verify_index_page.ps1
git commit -m "Add viewer source debug and shader explode contract"
```

---

### Task 9: Local Workflow Documentation

**Files:**
- Create: `docs/local_project_workflow.md`
- Modify: `README.md`
- Modify: `history.md`

- [ ] **Step 1: Write workflow doc**

Create `docs/local_project_workflow.md`:

```markdown
# Local Project Workflow

## Purpose

Use this workflow when a project arrives as many IFC/DGN/DWG/RVT files with mixed CRS, unknown unit scale, duplicates, 2D drawings, or suspicious geometry far outside the project area.

## Golden Rule

Never auto-merge every source. Every source must pass inspect and approval before publish, and every approved source must remain a separate normalized tileset before the root publish tileset references it.

Preferred project shape:

```text
normalized/
  bridge-a/
    tileset.json
    tileset_smooth_90.json
    tileset_smooth.json
  bridge-b/
    tileset.json
    tileset_smooth_90.json
    tileset_smooth.json

publish/
  tileset.json
  tileset_smooth_90.json
  tileset_smooth.json
  sources_manifest.json
  groups.json
  properties.sqlite
  properties_lookup.json
  warnings.json
```

## Stages

1. `inspect`: discover sources, write `source_manifest.json`, `cad_metadata/<source-id>.json`, `group_candidates.json`, duplicate candidates, and warnings.
2. `review`: user checks quarantined files, centroid/percentile bounds, scale candidates, duplicate candidates, CAD hierarchy dumps, and group candidates.
3. `convert-source`: approved sources are converted into normalized per-source tileset outputs.
4. `publish`: approved normalized outputs are wrapped into one root tileset set:
   - `tileset.json`
   - `tileset_smooth_90.json`
   - `tileset_smooth.json`
5. `viewer`: viewer reads one root tileset and can filter/explode by `source_id` and `explode_group_key`; property search reads `properties_lookup.json` for static demo mode or `properties.sqlite` through a local/API adapter in managed mode.

## Scale Rules

Allowed source unit scales:

- `1000.0`: kilometer-like or very small source coordinates that must be expanded to meters.
- `1.0`: EPSG:3826 meters.
- `0.1`: decimeter-like source coordinates.
- `0.01`: centimeter-like source coordinates.
- `0.001`: millimeter-like source coordinates.

Scale selection uses centroid and percentile bounds first. Raw bbox is retained for diagnostics, but a single stray point or construction line must not be allowed to reject an otherwise valid source by itself.

If none of these puts the source centroid and percentile bounds inside the project AOI, quarantine the source.

## Suspicious Source Rules

Quarantine when:

- Source centroid or percentile bounds are outside AOI for every allowed scale.
- Raw bbox is extremely large or far away, even when percentile bounds pass; this is a warning that requires review.
- Source appears almost 2D.
- Source size is physically impossible for the project.
- Source geometry fingerprint overlaps an approved source enough to look like a duplicate.
- Source lacks a credible conversion path.

## No Bentley Tool Assumption

The default route does not require paid Bentley tools. Existing IFC is the primary geometry path. DGN/DWG inspection is best effort through available local tools such as GDAL/OGR or an external converter, and missing tools are reported instead of silently skipped.

High-quality DGN conversion without Bentley tooling is not guaranteed. Parametric solids, smart objects, civil alignments, terrain, and OpenRoads metadata may be lost or flattened by non-Bentley routes. This workflow treats DGN/DWG data first as inspect/enrichment sources unless a reliable geometry export path is proven for the specific project.
```

- [ ] **Step 2: Update README**

Add a section:

```markdown
## Local Project Workspace

For many mixed IFC/DGN/DWG/RVT sources, use the local project workflow instead of directly merging every file. The workflow inspects sources, detects scale candidates `1000.0 / 1.0 / 0.1 / 0.01 / 0.001`, quarantines suspicious files, dumps CAD hierarchy metadata, and only publishes approved normalized sources.

See `docs/local_project_workflow.md`.
```

- [ ] **Step 3: Update history**

Add:

```markdown
### Local Project Ingest Plan

- Planned local-first workspace for mixed IFC/DGN/DWG/RVT project delivery.
- Required inspect before merge because sources may be EPSG:3826, EPSG:3826 scale 1000.0, 0.1, 0.01, or 0.001.
- CAD hierarchy dump is required for DGN/DWG because explode/filter/search may depend on reference, model, level, cell, shared cell, material, or element class.
- Suspicious sources outside AOI by centroid/percentile bounds, nearly 2D, raw-bbox polluted, or likely duplicate must be quarantined before publish.
```

- [ ] **Step 4: Commit**

```powershell
git add docs/local_project_workflow.md README.md history.md
git commit -m "Document local project ingest workflow"
```

---

### Task 10: End-To-End Verification Script

**Files:**
- Create: `tools/verify_project_workflow.ps1`

- [ ] **Step 1: Create verification script**

Create `tools/verify_project_workflow.ps1`:

```powershell
param(
  [Alias("Input")]
  [string]$InputPath = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\sample_files\淡江大橋移交模型",
  [string]$Output = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\inspect_tamkang"
)

$ErrorActionPreference = "Stop"

$cargo = @(
  "$env:USERPROFILE\.cargo\bin\cargo.exe",
  "$env:ProgramFiles\Rust stable MSVC 1.88\bin\cargo.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $cargo) {
  throw "找不到 cargo"
}

& $cargo test
if ($LASTEXITCODE -ne 0) {
  throw "cargo test failed"
}

& $cargo run -- inspect --input $InputPath --output $Output --source-epsg 3826
if ($LASTEXITCODE -ne 0) {
  throw "inspect command failed"
}

$manifest = Join-Path $Output "source_manifest.json"
$groups = Join-Path $Output "group_candidates.json"

if (-not (Test-Path -LiteralPath $manifest)) {
  throw "source_manifest.json 不存在"
}

if (-not (Test-Path -LiteralPath $groups)) {
  throw "group_candidates.json 不存在"
}

Write-Host "project workflow verification passed"
```

- [ ] **Step 2: Run verification**

Run:

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\verify_project_workflow.ps1
```

Expected:

- Rust tests pass.
- Inspect output exists.
- Verification prints `project workflow verification passed`.

- [ ] **Step 3: Commit**

```powershell
git add tools/verify_project_workflow.ps1
git commit -m "Add project workflow verification script"
```

---

### Task 11: Local Web Workstation Direction

**Files:**
- Create: `docs/local_web_workstation.md`
- Modify: `README.md`
- Modify: `history.md`

This task documents the product direction after inspect reports become trustworthy. It prevents the project from drifting into Rust GUI, Qt Desktop, Electron, or a generic single-page viewer. The target is a browser-based local workstation.

- [ ] **Step 1: Create workstation architecture doc**

Create `docs/local_web_workstation.md`:

```markdown
# Local Web Workstation

## Positioning

This project is a local web platform for BIM/GIS ingest, inspection, debugging, and publishing.

It is not:

- Rust GUI app
- Qt desktop app
- Electron all-in-one app
- React-heavy SaaS dashboard

## Responsibility Split

| Layer | Responsibility |
| --- | --- |
| Rust core | inspect, manifest, quarantine, geometry pipeline, publish, SQLite access, CLI, background worker |
| External tools | CAD/DGN/DWG conversion and probing |
| SQLite | durable source, feature, group, material, warning, transform, duplicate, and property records |
| Cesium | production GIS/BIM 3D Tiles viewer |
| Three.js | GLB/small-model debug viewer only |
| PHP dashboard or Axum API | local management UI/API over the same SQLite and manifest outputs |

## Backend Direction

First implementation target remains Rust CLI and worker. After inspect reports are trustworthy, expose the same project workspace through either:

- Rust + Axum local API for standalone local web mode.
- PHP dashboard adapter for existing 3wa-style deployment.

Preferred API shape:

```text
GET  /api/projects
GET  /api/sources
GET  /api/source/:id
GET  /api/features/:id
GET  /api/groups
POST /api/quarantine/approve
POST /api/quarantine/reject
POST /api/publish
```

## Frontend Direction

Use stable low-friction libraries:

| Function | Library |
| --- | --- |
| Layout | Bootstrap 5 |
| DOM/events | jQuery |
| Dock panels | GoldenLayout |
| Tables | Tabulator |
| Trees | jsTree |
| Viewer | Cesium |
| Charts | Chart.js |

Do not use React, Vue, Angular, Tailwind, MUI, or a large state-management stack for the first workstation.

## Workstation Layout

```text
+------------------------------------------------+
| toolbar                                        |
+---------+----------------------+---------------+
| source  |                      | metadata      |
| tree    |      Cesium          | properties    |
| group   |                      | warnings      |
+---------+----------------------+---------------+
| console / sql / progress / timeline            |
+------------------------------------------------+
```

Cesium must keep the largest screen area. Everything else supports inspection and debugging.

## Required Panels

- Source Tree
- Group Tree
- Feature Inspector
- Quarantine Queue
- AOI / CRS Debug Overlay
- SQL Panel
- Conversion Timeline
- Warnings / Console

## SQL Panel Contract

The SQL panel queries `properties.sqlite`.

Example:

```sql
SELECT feature_key, source_id, name, explode_group_key
FROM feature_properties
WHERE ifc_type = 'IfcCableSegment';
```

Query results must be highlightable in Cesium by `feature_key`.

## Product Priority

The first web workstation goal is inspect/debug, not visual polish.

The workflows that matter most:

- Which source has wrong scale.
- Which source bbox or percentile bounds are suspicious.
- Which source is duplicated.
- Which DGN/DWG hierarchy bucket should drive explode/filter/search.
- Which source is quarantined and why.
```

- [ ] **Step 2: Update README**

Add:

```markdown
## Local Web Workstation Direction

The preferred product shape is a local web platform: Rust core/worker, external CAD conversion tools, SQLite property database, and Cesium as the production viewer. Avoid Rust GUI, Qt Desktop, and Electron packaging for the first version.

See `docs/local_web_workstation.md`.
```

- [ ] **Step 3: Update history**

Add:

```markdown
### Local Web Workstation Direction

- Direction set to Local Web Platform / BIM-GIS workstation.
- Rust remains the trusted pipeline core.
- External tools handle DGN/DWG conversion/probing.
- SQLite stores durable property data.
- Cesium remains the core production viewer.
- Bootstrap + jQuery + GoldenLayout + Tabulator + jsTree are preferred for UI.
- Rust GUI, Qt Desktop, and Electron all-in-one packaging are explicitly deferred.
```

- [ ] **Step 4: Commit**

```powershell
git add docs/local_web_workstation.md README.md history.md docs/superpowers/plans/2026-05-19-local-project-ingest-publish.md
git commit -m "Document local web workstation direction"
```

---

## Final Verification

Run:

```powershell
cargo test
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\verify_index_page.ps1
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\verify_project_workflow.ps1
```

Expected:

- All Rust tests pass.
- Existing viewer checks still pass.
- Project workflow inspect produces:
  - `source_manifest.json`
  - `cad_metadata/<source-id>.json` for DGN/DWG sources, even when the buckets are empty because no local open probe is available.
  - `group_candidates.json`
  - duplicate candidate hints.
  - centroid/percentile bounds and quarantine/approval hints for mixed CRS/scale source handling.
  - publish debug manifests: `sources_manifest.json`, `groups.json`, `warnings.json`.
  - property database: `properties.sqlite` plus static viewer lookup `properties_lookup.json`.

## Rollout

1. First run against `sample_files\淡江大橋移交模型`.
2. First priority is DGN/DWG inspect dump; do this even before reliable DGN geometry conversion.
3. Review `source_manifest.json`, `cad_metadata/<source-id>.json`, duplicate candidates, and quarantined reasons manually.
4. Confirm scale detection for `1000.0 / 1.0 / 0.1 / 0.01 / 0.001`.
5. Confirm AOI logic uses centroid and percentile bounds; raw bbox only blocks publish when review confirms it is real model extent, not stray CAD garbage.
6. Confirm IFC group candidates are too coarse and DGN/DWG dump is needed for reference/model/level/cell/material grouping.
7. Convert only approved sources into separate `normalized/<source-id>/` tilesets.
8. Publish root tilesets after source review; root tilesets reference child tilesets and do not merge geometry buffers.
9. Treat Level 1 correctness as the acceptance gate: CRS, scale, canonical transform, centroid/percentile bounds, source identity, CAD hierarchy, group keys, group centers, material/color metadata, SQLite-backed property metadata, duplicate suppression, and quarantine decisions.
10. Track Level 2 fidelity separately: perfect geometry, perfect hierarchy, parametric solids, smart objects, civil alignment, terrain, and OpenRoads metadata.

## Self-Review

- Spec coverage: plan covers local workspace, DGN/DWG hierarchy inspect dump, scale normalization, canonical CRS/source transform, centroid/percentile AOI validation, duplicate fingerprinting, quarantine, group candidates, group spatial centers, SQLite property database, shader explode contract, per-source normalized tilesets, publish root wrapping, viewer debug metadata, docs, and verification.
- Placeholder scan: no placeholder tokens remain.
- Type consistency: `SourceFormat`, `SourceStatus`, `WorkspaceLayout`, `ProjectManifest`, `CadHierarchyDump`, `Bounds2`, `BoundsSummary`, `Aoi`, `SourceTransform`, `GeometryFingerprint`, `SourcePropertyRecord`, `FeaturePropertyRecord`, `PublishSource`, `SourceManifestEntry`, `GroupManifestEntry`, and metadata fields are introduced before later tasks use them.
