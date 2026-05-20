# Phase 2A Visual Geometry Quality Design

## Goal

Improve the approved-only minimal geometry preview so it is easier to understand visually and easier to regression-check. This phase does not repair CAD geometry fidelity and does not change the formal publish/runtime schema.

## Selected Approach

Use the **Visual + QA baseline** approach:

- Keep a single `geometry_preview/raw.glb` for Phase 2A.
- Improve Rust-side visual classification and colors.
- Add viewer-side controls and stats that make preview composition visible.
- Add screenshot baseline tooling for repeatable visual QA.

This avoids splitting the preview into multiple GLBs too early. If source/category hide/show needs true mesh-level isolation later, Phase 2B can split surfaces / lines / markers into separate GLBs.

## Backend Design

`src/geometry_preview.rs` remains the source of preview GLB generation.

Each approved entity preview gets a lightweight visual classification:

- `wall`
- `slab`
- `beam`
- `column`
- `annotation`
- `linework`
- `marker`
- `unknown`

Classification is rule-based using `geometry_type`, `layer`, and the existing Phase 1N cleanup action. It intentionally does not read full CAD materials or CAD hierarchy.

Color is assigned from the visual category:

- wall: cool blue
- slab: muted green
- beam: amber
- column: violet
- annotation: pale gray
- linework: yellow/orange
- marker: pink
- unknown: neutral gray

The publish report gains visual QA stats:

- `visual_category_counts`
- `line_width_exaggeration`
- `surface_shading_mode`
- `double_side_debug_available`

Phase 1N cleanup counts remain in the same report.

## Viewer Design

`src/publish_skeleton.rs` keeps loading the single preview GLB by default.

Viewer controls add a compact "Visual Preview" group:

- surfaces
- lines
- markers
- QA bbox
- pick overlay
- double-side debug

In Phase 2A, the category toggles drive QA/diagnostic overlays and status visibility. They do not attempt to surgically hide triangles inside the single GLB. True mesh-group hide/show is intentionally deferred to Phase 2B if needed.

The viewer stats overlay displays:

- triangle count
- line count
- surface count
- skipped tiny count
- debug marker count
- debug inflated count
- visual category histogram

## Screenshot Baseline

Add a script:

```powershell
tools/run_phase2a_preview_screenshot.ps1
```

The script opens the local publish viewer with `?phase2a=1`, fixes viewport/camera where possible, and writes:

```text
out/inspect_tamkang/publish/screenshots/phase2a_preview.png
out/inspect_tamkang/publish/screenshots/phase2a_visual_report.json
```

The first version only creates a stable baseline artifact. Pixel diff is deferred because Cesium, GPU drivers, WMTS availability, and browser DPI can create false positives.

## Non-Goals

- Do not split preview into multiple GLBs in Phase 2A.
- Do not implement production CAD mesh repair.
- Do not add Draco, meshopt, or LOD.
- Do not change `spatial_pick_index.json`.
- Do not change approved/rejected/needs_review publish gates.
- Do not add full CAD/BIM property tables to preview runtime.

## Acceptance Criteria

- The preview GLB uses category-aware colors instead of one-note proxy colors.
- The viewer can show preview stats and toggle QA/pick/diagnostic visual groups.
- `geometry_publish_report.json` exposes visual category counts and Phase 1N cleanup counts.
- A screenshot baseline script can produce a PNG and JSON report.
- Existing QA overlays, hybrid pick, and diagnostics continue to work.
- `cargo test`, `tools/run_phase1k_geometry_preview.ps1`, and `tools/verify_index_page.ps1` pass.
