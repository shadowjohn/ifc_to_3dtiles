# Phase 2A Visual Geometry Quality Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the approved-only minimal geometry preview visually understandable with category colors, visible linework, stats overlays, and screenshot baseline artifacts.

**Architecture:** Keep the Phase 1K/1N single-GLB preview pipeline. Rust assigns visual category/color/stat metadata while Cesium keeps loading `geometry_preview/raw.glb` and adds lightweight UI controls. Screenshot baseline is a local Playwright-powered script that writes artifacts under publish output.

**Tech Stack:** Rust CLI, serde JSON, existing `Mesh` GLB writer, Cesium 1.141 viewer HTML generated from `src/publish_skeleton.rs`, PowerShell, Playwright where available.

---

## File Structure

- Modify `src/geometry_preview.rs`
  - Add `VisualCategory` classification.
  - Route preview colors through category colors.
  - Add visual stats to `GeometryPublishReport`.
- Modify `src/publish_skeleton.rs`
  - Add Visual Preview UI controls.
  - Render `geometry_publish_report.json` stats.
  - Wire QA bbox / pick overlay toggles to existing overlay visibility paths.
- Create `tools/run_phase2a_preview_screenshot.ps1`
  - Launch screenshot capture script against the publish viewer.
- Create `tools/phase2a_preview_screenshot.mjs`
  - Use Playwright to capture a fixed-size screenshot and write JSON report.
- Modify `tests/project_pipeline.rs`
  - Add Rust behavior tests and static viewer/script tests.
- Modify `README.md`, `history.md`, `使用方式.md`
  - Document Phase 2A behavior and commands.

## Task 1: Backend Visual Category Stats

**Files:**
- Modify: `src/geometry_preview.rs`
- Test: `tests/project_pipeline.rs`

- [ ] **Step 1: Write failing tests**

Add tests to `tests/project_pipeline.rs`:

```rust
#[test]
fn phase2a_preview_report_counts_visual_categories_and_quality_settings() {
    let output = build_minimal_geometry_preview(
        "*",
        [292100.0, 2785200.0, 0.0],
        [121.42, 25.15, 0.0],
        [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ],
        &[
            GeometryPreviewFeature {
                feature_id: 1,
                source_id: "dwg-12d5f1b6".to_string(),
                layer: "A-WALL".to_string(),
                geometry_type: "POLYHEDRALSURFACE".to_string(),
                bbox: [292100.0, 2785200.0, 0.0, 292110.0, 2785210.0, 4.0],
            },
            GeometryPreviewFeature {
                feature_id: 2,
                source_id: "dwg-12d5f1b6".to_string(),
                layer: "Cable-Line".to_string(),
                geometry_type: "LINESTRING".to_string(),
                bbox: [292100.0, 2785200.0, 8.0, 292130.0, 2785200.0, 8.0],
            },
            GeometryPreviewFeature {
                feature_id: 3,
                source_id: "dwg-12d5f1b6".to_string(),
                layer: "Anno-Text".to_string(),
                geometry_type: "POINT".to_string(),
                bbox: [292105.0, 2785205.0, 2.0, 292105.0, 2785205.0, 2.0],
            },
        ],
    )
    .expect("visual preview");

    assert_eq!(output.report.visual_category_counts.get("wall"), Some(&1));
    assert_eq!(output.report.visual_category_counts.get("linework"), Some(&1));
    assert_eq!(output.report.visual_category_counts.get("annotation"), Some(&1));
    assert!(output.report.line_width_exaggeration >= 1.0);
    assert_eq!(output.report.surface_shading_mode, "category_color_with_normals");
    assert!(output.report.double_side_debug_available);
}
```

- [ ] **Step 2: Run focused tests and verify failure**

Run:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test phase2a_preview_report_counts --test project_pipeline
```

Expected: fail because `GeometryPublishReport` has no `visual_category_counts`, `line_width_exaggeration`, `surface_shading_mode`, or `double_side_debug_available`.

- [ ] **Step 3: Implement visual category fields**

Add to `GeometryPublishReport`:

```rust
pub visual_category_counts: BTreeMap<String, usize>,
pub line_width_exaggeration: f64,
pub surface_shading_mode: String,
pub double_side_debug_available: bool,
```

Add `VisualCategory` enum and helpers:

```rust
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
```

Use layer keyword matching:

```text
wall: wall, 墻, 牆
slab: slab, floor, deck, 版
beam: beam, girder, cable, 梁
column: column, pier, pile, tower, 柱, 墩, 塔
annotation: anno, text, dim, label, 註, 文字
linework: LINE/CURVE geometry
marker: Phase 1N keep_as_point_marker
```

- [ ] **Step 4: Run focused test and verify pass**

Run:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test phase2a_preview_report_counts --test project_pipeline
```

Expected: pass.

## Task 2: Category Colors and Line Visibility

**Files:**
- Modify: `src/geometry_preview.rs`
- Test: `tests/project_pipeline.rs`

- [ ] **Step 1: Write failing color test**

Add:

```rust
#[test]
fn phase2a_preview_mesh_uses_category_specific_colors() {
    let output = build_minimal_geometry_preview(
        "*",
        [0.0, 0.0, 0.0],
        [121.42, 25.15, 0.0],
        [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ],
        &[
            GeometryPreviewFeature {
                feature_id: 1,
                source_id: "s".to_string(),
                layer: "A-WALL".to_string(),
                geometry_type: "POLYHEDRALSURFACE".to_string(),
                bbox: [0.0, 0.0, 0.0, 4.0, 4.0, 3.0],
            },
            GeometryPreviewFeature {
                feature_id: 2,
                source_id: "s".to_string(),
                layer: "Cable-Line".to_string(),
                geometry_type: "LINESTRING".to_string(),
                bbox: [10.0, 0.0, 0.0, 20.0, 0.0, 0.0],
            },
        ],
    )
    .expect("preview colors");

    let first_color = output.mesh.colors[0];
    let second_feature_color = output.mesh.colors[36];
    assert_ne!(first_color, second_feature_color);
}
```

- [ ] **Step 2: Run focused test and verify failure**

Run:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test phase2a_preview_mesh_uses_category_specific_colors --test project_pipeline
```

Expected: fail if both features still use legacy colors.

- [ ] **Step 3: Route preview colors through category color helper**

Use a helper returning `[f32; 4]`:

```rust
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
```

- [ ] **Step 4: Run focused test and verify pass**

Run:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test phase2a_preview_mesh_uses_category_specific_colors --test project_pipeline
```

Expected: pass.

## Task 3: Viewer Visual Preview Controls and Stats

**Files:**
- Modify: `src/publish_skeleton.rs`
- Test: `tests/project_pipeline.rs`

- [ ] **Step 1: Write failing static viewer test**

Add:

```rust
#[test]
fn phase2a_publish_viewer_has_visual_quality_controls_and_stats() {
    let html = render_publish_viewer_html();

    assert!(html.contains("visualPreviewPanel"));
    assert!(html.contains("previewSurfacesToggle"));
    assert!(html.contains("previewLinesToggle"));
    assert!(html.contains("previewMarkersToggle"));
    assert!(html.contains("previewQaBboxToggle"));
    assert!(html.contains("previewPickOverlayToggle"));
    assert!(html.contains("doubleSideDebugToggle"));
    assert!(html.contains("updateGeometryPreviewStats"));
    assert!(html.contains("visual_category_counts"));
}
```

- [ ] **Step 2: Run focused test and verify failure**

Run:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test phase2a_publish_viewer_has_visual_quality_controls --test project_pipeline
```

Expected: fail because controls and stats helper do not exist.

- [ ] **Step 3: Add compact visual preview panel**

In `render_publish_viewer_html`, add the controls near the existing `minimal geometry preview` toggle:

```html
<div id="visualPreviewPanel" class="panel-row">
  <label><input id="previewSurfacesToggle" type="checkbox" checked> surfaces</label>
  <label><input id="previewLinesToggle" type="checkbox" checked> lines</label>
  <label><input id="previewMarkersToggle" type="checkbox" checked> markers</label>
  <label><input id="previewQaBboxToggle" type="checkbox" checked> QA bbox</label>
  <label><input id="previewPickOverlayToggle" type="checkbox" checked> pick overlay</label>
  <label><input id="doubleSideDebugToggle" type="checkbox"> double-side debug</label>
</div>
<pre id="geometryPreviewStats"></pre>
```

Add `updateGeometryPreviewStats(report)` that prints triangle, line, skipped, marker, inflated, and category histogram values from `geometry_publish_report.json`.

- [ ] **Step 4: Run focused test and verify pass**

Run:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test phase2a_publish_viewer_has_visual_quality_controls --test project_pipeline
```

Expected: pass.

## Task 4: Screenshot Baseline Tooling

**Files:**
- Create: `tools/run_phase2a_preview_screenshot.ps1`
- Create: `tools/phase2a_preview_screenshot.mjs`
- Test: `tests/project_pipeline.rs`

- [ ] **Step 1: Write failing script test**

Add:

```rust
#[test]
fn phase2a_screenshot_baseline_scripts_exist_and_write_expected_artifacts() {
    let ps1 = std::fs::read_to_string("tools/run_phase2a_preview_screenshot.ps1")
        .unwrap_or_default();
    let mjs = std::fs::read_to_string("tools/phase2a_preview_screenshot.mjs")
        .unwrap_or_default();

    assert!(ps1.contains("phase2a_preview_screenshot.mjs"));
    assert!(ps1.contains("phase2a_preview.png"));
    assert!(ps1.contains("phase2a_visual_report.json"));
    assert!(mjs.contains("phase2a_preview.png"));
    assert!(mjs.contains("phase2a_visual_report.json"));
    assert!(mjs.contains("geometryPreviewToggle"));
}
```

- [ ] **Step 2: Run focused test and verify failure**

Run:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test phase2a_screenshot_baseline_scripts --test project_pipeline
```

Expected: fail because scripts do not exist.

- [ ] **Step 3: Add PowerShell wrapper**

Create `tools/run_phase2a_preview_screenshot.ps1` with:

```powershell
param(
  [string]$Url = "http://127.0.0.1:8120/index.html?phase2a=1",
  [string]$PublishDir = "out\inspect_tamkang\publish"
)

$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent $PSScriptRoot
$Script = Join-Path $PSScriptRoot "phase2a_preview_screenshot.mjs"
$OutputDir = Join-Path $ProjectRoot (Join-Path $PublishDir "screenshots")
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

node $Script --url $Url --output-dir $OutputDir
if ($LASTEXITCODE -ne 0) {
  throw "phase2a screenshot capture failed with exit code $LASTEXITCODE"
}
```

- [ ] **Step 4: Add Node screenshot script**

Create `tools/phase2a_preview_screenshot.mjs` using Playwright dynamic import. The script must:

- parse `--url` and `--output-dir`
- open Chromium
- set viewport to `1440x900`
- wait for `#geometryPreviewToggle`
- save `phase2a_preview.png`
- write `phase2a_visual_report.json` with url, viewport, screenshot path, timestamp

- [ ] **Step 5: Run focused test and verify pass**

Run:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test phase2a_screenshot_baseline_scripts --test project_pipeline
```

Expected: pass.

## Task 5: Docs and Verification

**Files:**
- Modify: `README.md`
- Modify: `history.md`
- Modify: `使用方式.md`
- Verify: `tools/run_phase1k_geometry_preview.ps1`
- Verify: `tools/verify_index_page.ps1`

- [ ] **Step 1: Document Phase 2A**

Add concise usage:

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\run_phase1k_geometry_preview.ps1
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\run_phase2a_preview_screenshot.ps1
```

Mention:

- single-GLB preview remains.
- visual categories are rule-based.
- screenshot baseline is not pixel diff yet.

- [ ] **Step 2: Run formatter**

Run:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" fmt
```

Expected: exit 0.

- [ ] **Step 3: Run full tests**

Run:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test
```

Expected: all tests pass.

- [ ] **Step 4: Regenerate preview outputs**

Run:

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\run_phase1k_geometry_preview.ps1
```

Expected: `geometry_publish_report.json` contains visual category counts.

- [ ] **Step 5: Run viewer verify**

Run:

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\verify_index_page.ps1
```

Expected: viewer checks pass.

- [ ] **Step 6: Optional screenshot smoke**

Only run if the local viewer is available on `127.0.0.1:8120` and Playwright is installed:

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\run_phase2a_preview_screenshot.ps1
```

Expected:

```text
out\inspect_tamkang\publish\screenshots\phase2a_preview.png
out\inspect_tamkang\publish\screenshots\phase2a_visual_report.json
```

## Self-Review

- Spec coverage: all requested Phase 2A items are mapped to tasks except true mesh-level hide/show, which is explicitly deferred because the current output is one GLB.
- Placeholder scan: no task uses TBD/TODO language.
- Type consistency: report fields use existing Rust snake_case JSON naming; viewer checks look for generated JSON keys.
