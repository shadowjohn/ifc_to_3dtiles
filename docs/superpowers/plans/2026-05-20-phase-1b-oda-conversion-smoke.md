# Phase 1B ODA Conversion Smoke Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a trustworthy ODA-based CAD conversion smoke pipeline that converts each DGN/DWG into normalized CAD outputs, then re-inspects bbox, scale, and duplicate signals.

**Architecture:** OGR is no longer the primary route for this sample set because it cannot open the current DGN/DWG reliably. Rust remains the trusted orchestrator, ODA File Converter is the CAD conversion tool, and inspect reports compare original source identity against converted outputs. The first target is report credibility, not 3D Tiles publish.

**Tech Stack:** Rust CLI, PowerShell ODA runner, ODA File Converter 27.1.0 preferred with 20.12 fallback, serde JSON reports, existing inspect/georef/fingerprint modules.

---

## Phase Boundary

Phase 1A is complete:

- Source discovery inspect exists.
- CAD metadata sidecars exist with stable empty buckets.
- Source IDs are unique even for Chinese filenames.
- ODA 27.1.0 is detected as preferred.
- OGR has been proven unreliable for the current DGN/DWG sample set.

Phase 1B does not publish 3D Tiles. It only proves whether ODA-converted CAD can be inspected reliably enough for bbox, scale, hierarchy, and duplicate decisions.

## Baseline Decision

Keep tool version and exported CAD version separate:

- `oda_version`: the actual ODA File Converter binary version. Current accepted baseline is `27.1.0.0` from `C:\Program Files\ODA\ODAFileConverter 27.1.0\ODAFileConverter.exe`.
- `target_version`: the CAD output version requested from ODA. First smoke target is `ACAD2018`; if OGR still cannot inspect the output, try `ACAD2013` and DXF targets in the next run.
- `target_format`: `DWG` first, `DXF` fallback only when the post-ODA DWG is still not inspectable.

Do not select the old `20.12.0.0` converter when `27.1.0.0` is available. It can remain in the report only as fallback evidence.

## Input Manifest Additions

Before ODA conversion, make `source_manifest.json` easier to debug by preserving human-readable source identity:

```json
{
  "id": "dwg-850173d8",
  "display_name": "主橋",
  "original_file_name": "主橋.dwg",
  "relative_path": "主橋.dwg",
  "path": "C:\\Users\\stw_s\\Desktop\\ifc_to_3dtiles\\sample_files\\淡江大橋移交模型\\主橋.dwg",
  "format": "dwg",
  "warnings": []
}
```

Also flag likely intermediate/export-copy CAD names before conversion:

```json
{
  "id": "djb-m-su-dgn-i-dgn-c3cd6d29",
  "original_file_name": "DJB-M-SU-監測.dgn.i.dgn",
  "warnings": [
    "possible_intermediate_or_export_copy: file has repeated CAD-like extensions"
  ]
}
```

This does not quarantine the file. It only makes later duplicate and overlap investigation easier.

## Output Contract

Each converted CAD source writes one report entry:

```json
{
  "source_id": "djb-m-su-dwg-a1b2c3d4",
  "source_display_name": "DJB-M-SU-監測",
  "source_original_file_name": "DJB-M-SU-監測.dwg",
  "source_relative_path": "DJB-M-SU-監測.dwg",
  "input_path": "C:\\Users\\stw_s\\Desktop\\ifc_to_3dtiles\\sample_files\\淡江大橋移交模型\\DJB-M-SU-監測.dwg",
  "input_format": "dwg",
  "converted_path": "C:\\Users\\stw_s\\Desktop\\ifc_to_3dtiles\\out\\oda_normalized\\djb-m-su-dwg-a1b2c3d4\\DJB-M-SU-監測_R2018.dwg",
  "converted_format": "dwg",
  "oda_version": "27.1.0.0",
  "target_version": "ACAD2018",
  "target_format": "DWG",
  "success": true,
  "input_sha256": "hex",
  "converted_sha256": "hex",
  "bbox_before": null,
  "bbox_after": {
    "raw": null,
    "percentile": null
  },
  "level_count_after": null,
  "material_count_after": null,
  "fingerprint_after": null,
  "warnings": []
}
```

Final report path:

```text
out/oda_normalized/oda_conversion_report.json
```

## Task 0: Source Manifest Usability Fields

**Files:**
- Modify: `src/project.rs`
- Modify: `src/inspect.rs`
- Test: `tests/project_pipeline.rs`

- [ ] **Step 1: Write failing tests for readable source identity**

Add tests to `tests/project_pipeline.rs`:

```rust
#[test]
fn discover_sources_preserves_human_readable_identity() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let nested = tmp.path().join("cad").join("stage");
    std::fs::create_dir_all(&nested).expect("nested dir");
    std::fs::write(nested.join("主橋.dwg"), "dwg").expect("dwg");

    let sources = discover_sources(tmp.path()).expect("discover sources");
    let source = sources
        .iter()
        .find(|source| source.original_file_name == "主橋.dwg")
        .expect("source");

    assert_eq!(source.display_name, "主橋");
    assert_eq!(source.original_file_name, "主橋.dwg");
    assert_eq!(
        source.relative_path,
        std::path::PathBuf::from("cad").join("stage").join("主橋.dwg")
    );
}

#[test]
fn discover_sources_flags_repeated_cad_like_extensions() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(tmp.path().join("DJB-M-SU-監測.dgn.i.dgn"), "dgn").expect("dgn");

    let sources = discover_sources(tmp.path()).expect("discover sources");
    let source = sources
        .iter()
        .find(|source| source.original_file_name == "DJB-M-SU-監測.dgn.i.dgn")
        .expect("source");

    assert!(
        source
            .warnings
            .iter()
            .any(|warning| warning.contains("possible_intermediate_or_export_copy"))
    );
}
```

- [ ] **Step 2: Run tests and verify they fail**

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --test project_pipeline discover_sources_
```

Expected: fail because `SourceRecord` does not yet expose `display_name`, `original_file_name`, or `relative_path`.

- [ ] **Step 3: Add fields to `SourceRecord`**

Modify `src/project.rs`:

```rust
pub struct SourceRecord {
    pub id: String,
    pub display_name: String,
    pub original_file_name: String,
    pub relative_path: PathBuf,
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
```

- [ ] **Step 4: Update existing `SourceRecord` literals**

Update every `SourceRecord { ... }` literal in `tests/project_pipeline.rs` so it includes:

```rust
display_name: "DJB-M-SU-監測.dgn.i".to_string(),
original_file_name: "DJB-M-SU-監測.dgn.i.dgn".to_string(),
relative_path: PathBuf::from(r"sources\DJB-M-SU-監測.dgn.i.dgn"),
```

For manifests serialized in tests, assert the JSON has these fields and that old fields are still present.

- [ ] **Step 5: Populate fields during discovery**

In `src/inspect.rs`, build these values beside `stable_source_id(root, &path)`:

```rust
let relative_path = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
let original_file_name = path
    .file_name()
    .and_then(|name| name.to_str())
    .unwrap_or("source")
    .to_string();
let display_name = path
    .file_stem()
    .and_then(|name| name.to_str())
    .unwrap_or(&original_file_name)
    .to_string();
let mut warnings = Vec::new();
if has_repeated_cad_like_extension(&original_file_name) {
    warnings.push(
        "possible_intermediate_or_export_copy: file has repeated CAD-like extensions".to_string(),
    );
}
```

Add helper:

```rust
fn has_repeated_cad_like_extension(file_name: &str) -> bool {
    let lowered = file_name.to_ascii_lowercase();
    let cad_tokens = [".dgn.", ".dwg.", ".dxf.", ".ifc."];
    cad_tokens.iter().any(|token| lowered.contains(token))
}
```

- [ ] **Step 6: Run targeted tests**

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --test project_pipeline discover_sources_
```

Expected: pass.

- [ ] **Step 7: Commit**

```powershell
git add src/project.rs src/inspect.rs tests/project_pipeline.rs
git commit -m "Preserve readable source identity in manifest"
```

## Task 1: Lock ODA Baseline

**Files:**
- Modify: `tools/inspect_cad_sources.ps1`
- Modify: `src/inspect.rs`
- Test: `tests/project_pipeline.rs`

- [ ] **Step 1: Add test for preferred ODA baseline**

Add test asserting:

- `preferred_oda_file_converter.version == "27.1.0.0"`
- `version_risk == "acceptable_baseline"`
- old `20.12.0.0` may exist but must not be selected when 27.1 exists.

- [ ] **Step 2: Run test**

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --test project_pipeline cad_probe_summary_marks_old_and_preferred_oda_converter
```

Expected: pass.

- [ ] **Step 3: Add `--require-oda-major 27` to probe script**

If preferred ODA major is below the requested major, script exits non-zero with:

```text
Preferred ODA File Converter is too old
```

- [ ] **Step 4: Verify**

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\inspect_cad_sources.ps1 --require-oda-major 27
```

Expected: preferred ODA is 27.1.0.0 and command succeeds.

- [ ] **Step 5: Commit**

```powershell
git add tools/inspect_cad_sources.ps1 src/inspect.rs tests/project_pipeline.rs
git commit -m "Lock ODA baseline for CAD conversion"
```

## Task 2: ODA Conversion Smoke Runner

**Files:**
- Create: `tools/convert_cad_with_oda.ps1`
- Create: `src/cad_conversion.rs`
- Modify: `src/lib.rs`
- Test: `tests/project_pipeline.rs`

- [ ] **Step 1: Write report schema test**

Add a Rust test that serializes and deserializes `CadConversionReportEntry` with the exact output contract above.

- [ ] **Step 2: Implement report structs**

Create `src/cad_conversion.rs` with:

- `CadConversionReport`
- `CadConversionReportEntry`
- `CadConversionStatus`

Fields must match the output contract. Use `Option<Value>` for bbox/fingerprint placeholders until converted CAD inspection is proven.

- [ ] **Step 3: Create PowerShell runner**

Create `tools/convert_cad_with_oda.ps1`.

Inputs:

```powershell
param(
  [string]$Manifest = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\inspect_tamkang\source_manifest.json",
  [string]$Output = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\oda_normalized",
  [string]$TargetVersion = "ACAD2018",
  [string]$TargetFormat = "DWG"
)
```

Behavior:

- Read `source_manifest.json`.
- Process only `format=dgn` or `format=dwg`.
- Use preferred ODA from `out/cad_probe/cad_probe_report.json`.
- Write each converted source under `out/oda_normalized/<source_id>/`.
- Write `oda_conversion_report.json`.
- Preserve input source identity and SHA256.
- Copy source readability fields into each entry: `source_display_name`, `source_original_file_name`, `source_relative_path`.
- Store both ODA tool version and requested output version: `oda_version`, `target_version`, `target_format`.

- [ ] **Step 4: Smoke run one source first**

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\convert_cad_with_oda.ps1 -Limit 1
```

Expected:

- One source is attempted.
- Report has one entry.
- Failure is acceptable only if report captures command, exit code, and warning.

- [ ] **Step 5: Commit**

```powershell
git add tools/convert_cad_with_oda.ps1 src/cad_conversion.rs src/lib.rs tests/project_pipeline.rs
git commit -m "Add ODA conversion smoke runner"
```

## Task 3: Convert All DGN/DWG To Normalized CAD

**Files:**
- Modify: `tools/convert_cad_with_oda.ps1`
- Modify: `history.md`

- [ ] **Step 1: Run all-source conversion**

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\convert_cad_with_oda.ps1
```

Expected:

- Seven CAD sources are attempted.
- Each source has exactly one report entry.
- Converted outputs, if any, are under `out/oda_normalized/<source_id>/`.

- [ ] **Step 2: Verify report completeness**

```powershell
$report = Get-Content .\out\oda_normalized\oda_conversion_report.json | ConvertFrom-Json
if (($report.entries | Measure-Object).Count -ne 7) { throw "expected 7 CAD conversion entries" }
```

- [ ] **Step 3: Commit script/report metadata only**

Do not commit converted CAD files. Commit only source code, scripts, docs, and history.

```powershell
git add tools/convert_cad_with_oda.ps1 history.md
git commit -m "Run ODA conversion smoke for CAD sources"
```

## Task 4: Re-Inspect Converted CAD

**Files:**
- Create: `tools/inspect_normalized_cad.ps1`
- Modify: `src/cad_conversion.rs`
- Test: `tests/project_pipeline.rs`

- [ ] **Step 1: Create re-inspect script**

Create `tools/inspect_normalized_cad.ps1`:

- Reads `out/oda_normalized/oda_conversion_report.json`.
- Runs `ogrinfo -so` against successful converted outputs.
- Captures bbox / layer names / material-like fields if available.
- Writes `out/oda_normalized/normalized_cad_inspect_report.json`.

- [ ] **Step 2: Compare before/after**

For each entry, write:

- `bbox_before`
- `bbox_after`
- `scale_candidates_after`
- `level_count_after`
- `material_count_after`
- `warnings`

- [ ] **Step 3: Verify OGR after ODA**

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\inspect_normalized_cad.ps1
```

Acceptance:

- If OGR can inspect converted files, proceed to scale/fingerprint.
- If OGR still fails, report must say ODA conversion is not enough for OGR-based inspect and next route is ODA-derived DXF or a lower DWG target such as `ACAD2013`.

- [ ] **Step 4: Commit**

```powershell
git add tools/inspect_normalized_cad.ps1 src/cad_conversion.rs tests/project_pipeline.rs
git commit -m "Add normalized CAD re-inspect report"
```

## Final Verification

Run:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\inspect_cad_sources.ps1 --require-oda-major 27
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\convert_cad_with_oda.ps1
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\inspect_normalized_cad.ps1
```

Expected:

- Rust tests pass.
- ODA 27.1.0 is preferred.
- Seven CAD sources have conversion report entries.
- Converted CAD inspect report makes a clear decision:
  - ODA → DWG is inspectable, or
  - ODA → DXF / lower DWG is needed next.
