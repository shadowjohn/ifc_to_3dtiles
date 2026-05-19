# Local Project Ingest And Publish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a local-first project workspace that can inspect many IFC/DGN/DWG/RVT sources, detect CRS/scale problems, quarantine suspicious files, enrich metadata/group candidates, and publish one easy-to-load 3D Tiles package with flat / 90 / 180 normal modes.

**Architecture:** Keep the existing Rust IFC/RVT converter as the geometry core, then add an ingest layer before conversion and a publish layer after conversion. The ingest layer writes deterministic manifests and reports; the publish layer combines approved normalized sources into root tilesets and viewer-ready group metadata without silently merging bad files.

**Tech Stack:** Rust CLI, serde JSON manifests, existing IFC STEP parser, optional GDAL/OGR CLI probes for DGN/DWG inspection, PowerShell helper scripts, Cesium / Three viewer metadata integration.

---

## Scope

This plan intentionally does not build a cloud platform yet. It builds a local project workspace and local web/viewer-friendly outputs first, because the hard problem is data trust: CRS, scale, duplicates, wrong-country geometry, 2D drawings, missing materials, and group semantics.

Out of scope for this phase:

- Direct Bentley DGN kernel implementation in Rust.
- Paid Bentley / MicroStation / OpenRoads automation.
- Cloud job queue or multi-user database.
- Draco / meshopt compression.
- Replacing Cesium production viewer.

## File Structure

- Create `src/project.rs`: project workspace layout, manifest structs, source status enums.
- Create `src/inspect.rs`: inspect IFC/DGN/DWG/RVT files and produce per-source bbox, CRS, unit scale candidates, warnings.
- Create `src/georef.rs`: scale detection and AOI/anchor validation logic for EPSG:3826, `scale=1.0/0.1/0.01`.
- Create `src/grouping.rs`: derive group candidates from IFC metadata and future DGN/DWG dumps.
- Create `src/publish.rs`: combine approved source outputs into one root publish folder with three normal-mode tilesets.
- Modify `src/main.rs`: add subcommands `inspect`, `convert-source`, `publish`, keep current direct conversion behavior.
- Modify `src/lib.rs`: export new modules.
- Modify `src/convert.rs`: expose per-source conversion hooks and include new source/group fields in metadata.
- Create `tests/project_pipeline.rs`: manifest/georef/group/publish tests.
- Create `tools/inspect_cad_sources.ps1`: optional local helper that checks availability of `ogrinfo`, `ogr2ogr`, ODA/other CLI tools without requiring them.
- Create `docs/local_project_workflow.md`: operator workflow and failure handling.
- Modify `.gitignore`: ignore `sample_files/`, `*.dgn`, `*.dwg`, and generated project workspaces.

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
        allowed_scales: vec![1.0, 0.1, 0.01],
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
        allowed_scales: vec![1.0, 0.1, 0.01],
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

### Task 3: Georef Scale Classification

**Files:**
- Create: `src/georef.rs`
- Modify: `src/lib.rs`
- Modify: `src/inspect.rs`
- Test: `tests/project_pipeline.rs`

- [ ] **Step 1: Write failing tests for scale selection**

Add to `tests/project_pipeline.rs`:

```rust
use ifc_to_3dtiles::georef::{Aoi, Bounds2, classify_source_scale};

#[test]
fn scale_classifier_accepts_taiwan_epsg_3826_meter_bounds() {
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let bounds = Bounds2::new(300_000.0, 2_787_000.0, 301_000.0, 2_788_000.0);
    let result = classify_source_scale(bounds, &aoi, &[1.0, 0.1, 0.01]);
    assert_eq!(result.selected_scale, Some(1.0));
    assert_eq!(result.status, "inside_aoi");
}

#[test]
fn scale_classifier_detects_centimeter_like_coordinates() {
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let bounds = Bounds2::new(30_000_000.0, 278_700_000.0, 30_100_000.0, 278_800_000.0);
    let result = classify_source_scale(bounds, &aoi, &[1.0, 0.1, 0.01]);
    assert_eq!(result.selected_scale, Some(0.01));
    assert_eq!(result.status, "inside_aoi");
}

#[test]
fn scale_classifier_quarantines_far_away_model() {
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let bounds = Bounds2::new(4_000_000.0, 6_000_000.0, 4_100_000.0, 6_100_000.0);
    let result = classify_source_scale(bounds, &aoi, &[1.0, 0.1, 0.01]);
    assert_eq!(result.selected_scale, None);
    assert_eq!(result.status, "outside_aoi");
    assert!(result.warnings.iter().any(|w| w.contains("outside AOI")));
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```powershell
cargo test scale_classifier_accepts_taiwan_epsg_3826_meter_bounds scale_classifier_detects_centimeter_like_coordinates scale_classifier_quarantines_far_away_model
```

Expected: fail because `georef` module does not exist.

- [ ] **Step 3: Implement scale classifier**

Create `src/georef.rs`:

```rust
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScaleClassification {
    pub selected_scale: Option<f64>,
    pub status: String,
    pub warnings: Vec<String>,
}

pub fn classify_source_scale(
    raw_bounds: Bounds2,
    aoi: &Aoi,
    allowed_scales: &[f64],
) -> ScaleClassification {
    for scale in allowed_scales {
        let scaled = raw_bounds.scaled(*scale);
        if aoi.contains_bounds(scaled) && scaled.width() > 0.01 && scaled.height() > 0.01 {
            return ScaleClassification {
                selected_scale: Some(*scale),
                status: "inside_aoi".to_string(),
                warnings: vec![],
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
cargo test scale_classifier_accepts_taiwan_epsg_3826_meter_bounds scale_classifier_detects_centimeter_like_coordinates scale_classifier_quarantines_far_away_model
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

### Task 5: Quarantine Rules And Source Approval

**Files:**
- Modify: `src/project.rs`
- Modify: `src/georef.rs`
- Modify: `src/main.rs`
- Test: `tests/project_pipeline.rs`

- [ ] **Step 1: Write failing tests for quarantine decisions**

Add to `tests/project_pipeline.rs`:

```rust
use ifc_to_3dtiles::georef::{Aoi, Bounds2, decide_source_status};
use ifc_to_3dtiles::project::SourceStatus;

#[test]
fn decide_source_status_approves_inside_aoi_3d_source() {
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let status = decide_source_status(
        Bounds2::new(300_000.0, 2_787_000.0, 301_000.0, 2_788_000.0),
        20.0,
        &aoi,
        &[1.0, 0.1, 0.01],
    );
    assert_eq!(status.status, SourceStatus::Approved);
    assert_eq!(status.selected_scale, Some(1.0));
}

#[test]
fn decide_source_status_quarantines_flat_2d_source() {
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let status = decide_source_status(
        Bounds2::new(300_000.0, 2_787_000.0, 301_000.0, 2_788_000.0),
        0.001,
        &aoi,
        &[1.0, 0.1, 0.01],
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
    raw_bounds_xy: Bounds2,
    z_range_m: f64,
    aoi: &Aoi,
    allowed_scales: &[f64],
) -> SourceStatusDecision {
    let scale = classify_source_scale(raw_bounds_xy, aoi, allowed_scales);
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

### Task 6: CAD Tool Probe Script

**Files:**
- Create: `tools/inspect_cad_sources.ps1`
- Modify: `docs/local_project_workflow.md`

- [ ] **Step 1: Create tool probe script**

Create `tools/inspect_cad_sources.ps1`:

```powershell
param(
  [string]$Input = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\sample_files\淡江大橋移交模型",
  [string]$Output = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\cad_probe"
)

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $Output | Out-Null

function Find-CommandPath {
  param([string]$Name)
  $cmd = Get-Command $Name -ErrorAction SilentlyContinue
  if ($cmd) { return $cmd.Source }
  return $null
}

$tools = [ordered]@{
  ogrinfo = Find-CommandPath "ogrinfo"
  ogr2ogr = Find-CommandPath "ogr2ogr"
  odaconvert = Find-CommandPath "ODAFileConverter"
}

$files = Get-ChildItem -LiteralPath $Input -Recurse -File |
  Where-Object { $_.Extension -match '^\.(dgn|dwg)$' } |
  Select-Object FullName, Extension, Length

$report = [ordered]@{
  input = $Input
  output = $Output
  tools = $tools
  files = $files
  note = "No paid Bentley tools are required by this probe. Missing tools are reported, not installed."
}

$reportPath = Join-Path $Output "cad_probe_report.json"
$report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $reportPath -Encoding UTF8
Write-Host $reportPath
```

- [ ] **Step 2: Run script**

Run:

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\inspect_cad_sources.ps1
```

Expected:

- `out\cad_probe\cad_probe_report.json` exists.
- Missing GDAL/ODA tools are recorded as `null` instead of failing.

- [ ] **Step 3: Commit**

```powershell
git add tools/inspect_cad_sources.ps1
git commit -m "Add CAD source probe script"
```

---

### Task 7: Publish Root Tilesets For Approved Sources

**Files:**
- Create: `src/publish.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs`
- Test: `tests/project_pipeline.rs`

- [ ] **Step 1: Write failing tests for publish root tileset list**

Add to `tests/project_pipeline.rs`:

```rust
use ifc_to_3dtiles::publish::{PublishSource, build_publish_tileset};

#[test]
fn publish_tileset_wraps_child_tilesets_with_source_metadata() {
    let sources = vec![
        PublishSource {
            source_id: "main-ifc".to_string(),
            label: "DJB-M-SU-監測.ifc".to_string(),
            tileset_uri: "sources/main-ifc/tileset.json".to_string(),
            normal_mode: "flat".to_string(),
        },
        PublishSource {
            source_id: "main-ifc".to_string(),
            label: "DJB-M-SU-監測.ifc".to_string(),
            tileset_uri: "sources/main-ifc/tileset_smooth_90.json".to_string(),
            normal_mode: "smooth_90".to_string(),
        },
    ];
    let tileset = build_publish_tileset(&sources, "flat");
    assert_eq!(tileset["asset"]["version"], "1.0");
    assert_eq!(tileset["root"]["children"].as_array().unwrap().len(), 1);
    assert_eq!(tileset["root"]["children"][0]["extras"]["source_id"], "main-ifc");
}
```

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
            })
        })
        .collect();
    fs::write(output.join("tileset.json"), serde_json::to_vec_pretty(&ifc_to_3dtiles::publish::build_publish_tileset(&sources, "flat"))?)?;
    fs::write(output.join("tileset_smooth_90.json"), serde_json::to_vec_pretty(&ifc_to_3dtiles::publish::build_publish_tileset(&sources, "smooth_90"))?)?;
    fs::write(output.join("tileset_smooth.json"), serde_json::to_vec_pretty(&ifc_to_3dtiles::publish::build_publish_tileset(&sources, "smooth"))?)?;
    println!("{}", output.display());
    return Ok(());
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

Never auto-merge every source. Every source must pass inspect and approval before publish.

## Stages

1. `inspect`: discover sources, write `source_manifest.json`, `group_candidates.json`, and warnings.
2. `review`: user checks quarantined files, scale candidates, duplicate candidates, and group candidates.
3. `convert-source`: approved sources are converted into normalized per-source outputs.
4. `publish`: approved normalized outputs are wrapped into one root tileset set:
   - `tileset.json`
   - `tileset_smooth_90.json`
   - `tileset_smooth.json`
5. `viewer`: viewer reads one root tileset and can filter/explode by `source_id` and `explode_group_key`.

## Scale Rules

Allowed source unit scales:

- `1.0`: EPSG:3826 meters.
- `0.1`: decimeter-like source coordinates.
- `0.01`: centimeter-like source coordinates.

If none of these puts the source inside the project AOI, quarantine the source.

## Suspicious Source Rules

Quarantine when:

- Source bbox is outside AOI for every allowed scale.
- Source appears almost 2D.
- Source size is physically impossible for the project.
- Source overlaps an approved source enough to look like a duplicate.
- Source lacks a credible conversion path.

## No Bentley Tool Assumption

The default route does not require paid Bentley tools. Existing IFC is the primary geometry path. DGN/DWG inspection is best effort through available local tools such as GDAL/OGR or an external converter, and missing tools are reported instead of silently skipped.
```

- [ ] **Step 2: Update README**

Add a section:

```markdown
## Local Project Workspace

For many mixed IFC/DGN/DWG/RVT sources, use the local project workflow instead of directly merging every file. The workflow inspects sources, detects scale candidates `1.0 / 0.1 / 0.01`, quarantines suspicious files, and only publishes approved normalized sources.

See `docs/local_project_workflow.md`.
```

- [ ] **Step 3: Update history**

Add:

```markdown
### Local Project Ingest Plan

- Planned local-first workspace for mixed IFC/DGN/DWG/RVT project delivery.
- Required inspect before merge because sources may be EPSG:3826, EPSG:3826 scale 0.1, or EPSG:3826 scale 0.01.
- Suspicious sources outside AOI, nearly 2D, or likely duplicate must be quarantined before publish.
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
  [string]$Input = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\sample_files\淡江大橋移交模型",
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

& $cargo run -- inspect --input $Input --output $Output --source-epsg 3826
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
  - `group_candidates.json`
  - quarantine/approval hints for mixed CRS/scale source handling.

## Rollout

1. First run against `sample_files\淡江大橋移交模型`.
2. Review `source_manifest.json` manually.
3. Confirm scale detection for `1.0 / 0.1 / 0.01`.
4. Confirm IFC group candidates are too coarse and DGN/DWG dump is needed only for enrichment.
5. Convert only approved sources.
6. Publish root tilesets after source review.

## Self-Review

- Spec coverage: plan covers local workspace, inspect, scale normalization, quarantine, group candidates, publish, metadata, docs, and verification.
- Placeholder scan: no placeholder tokens remain.
- Type consistency: `SourceFormat`, `SourceStatus`, `WorkspaceLayout`, `ProjectManifest`, `Bounds2`, `Aoi`, `PublishSource`, and metadata fields are introduced before later tasks use them.
