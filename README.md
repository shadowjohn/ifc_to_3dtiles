# IFC / RVT to GLB and Cesium 3D Tiles

Rust CLI for converting IFC2X3 models into standalone GLB plus Cesium 3D Tiles 1.0 (`tileset.json` + `.b3dm`). RVT input is supported through a Revit add-in bridge that exports IFC with Autodesk's Revit IFC exporter first.

目前目標是先把 `DJB-M-SU-_.ifc` 這類 AECOsim IFC2X3 橋梁模型穩定轉出可在 CesiumJS 載入、可點選、保留屬性、顏色正常的 3D Tiles。這不是完整通用 IFC kernel。

## Features

- STEP/IFC2X3 indexing parser, focused on AECOsim `FacetedBrep` / `MappedRepresentation`.
- RVT -> IFC -> GLB orchestration for local Revit 2025 / 2026 / 2027.
- Product metadata to Batch Table: IFC id、GlobalId、類型、名稱、樓層、群組、style、顏色、Pset JSON。
- IFC style color extraction with fallback report.
- Standalone `<name>_flat.glb` / `<name>_smooth.glb` with glTF `extras` metadata and metadata file pointer.
- `metadata.json` and `unsupported_geometry_report.json` beside generated tiles.
- EPSG:3826 to WGS84/ECEF georeferencing via `proj4rs`.
- Root ENU-to-ECEF transform with local float32 geometry.
- Spatial tiling with configurable feature / triangle limits.
- Flat and smooth normal output modes.
- Cesium demo viewer: select, focus display, explode, move-out pad, measurement, basemap switching, render scale control.
- Three.js comparison viewers:
  - pure Three.js + `3d-tiles-renderer`
  - MapLibre GL JS + Three.js custom layer + `3d-tiles-renderer`
- CAD inspect pipeline for DWG delivery:
  - ODA File Converter 27.1 baseline
  - DWG -> ACAD2000 / DXF
  - OGR entity-level geometry inspect
  - JSON reports plus SQLite inspect DB
  - Static Phase 1D review report for quarantine / duplicate QA
  - Phase 1G spatial QA manifest and Cesium interaction overlay for AOI、duplicate、outlier review

## Repository Policy

Git 只收可維護的 source / test / tool / doc：

- `src/`
- `tests/`
- `tools/`
- `Cargo.toml`
- `Cargo.lock`
- `out/DJB-M-SU-_/index.html`
- `out/DJB-M-SU-_/index_three.html`
- `out/DJB-M-SU-_/index_maplibre_three.html`
- `out/DJB-M-SU-_/three_viewer_common.js`
- docs and license

不收大型或可再生資料：

- `*.ifc`
- `target/`
- `out/**` generated tiles, tilesets, reports
- local `Cesium-1.141` distribution
- local Three / MapLibre / 3d-tiles-renderer `vendor/`

`DJB-M-SU-_.ifc` 目前約 117MB，且可能含專案資料，不進 git。需要轉檔時放在專案根目錄或另外指定路徑即可。

## Quick Start

```powershell
cd C:\Users\stw_s\Desktop\ifc_to_3dtiles
cargo build --release

.\target\release\ifc_to_3dtiles.exe `
  --input .\DJB-M-SU-_.ifc `
  --output .\out `
  --source-epsg 3826 `
  --tile-max-features 125 `
  --tile-max-triangles 40000 `
  --normal-mode both `
  --smooth-angle-deg 90 `
  --overwrite
```

注意：`--output` 請填 parent folder，例如 `.\out`。程式會自動建立 `out\<ifc-name>\`。如果填 `out\DJB-M-SU-_`，會變成 `out\DJB-M-SU-_\DJB-M-SU-_`。

## RVT Input

RVT 需要本機合法 Revit 2025-2027，並先建 Revit bridge：

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\build_revit_bridge.ps1 -Version 2026 -Configuration Release
```

轉檔：

```powershell
.\target\release\ifc_to_3dtiles.exe `
  --input ".\阻尼器(比例需調整).rvt" `
  --output .\out `
  --normal-mode both `
  --revit-version auto `
  --keep-ifc `
  --overwrite
```

細節見 [docs/rvt_revit_ifc.md](docs/rvt_revit_ifc.md)。

找不到 Revit 時，console 會提示官方安裝入口：

- Autodesk Account: <https://manage.autodesk.com/products>
- Revit Free Trial: <https://www.autodesk.com/products/revit/free-trial>

非預設安裝路徑可用 `--revit-exe "D:\...\Revit.exe" --revit-version 2026` 指定。

## Demo Viewer

Demo viewer 在：

```text
out/DJB-M-SU-_/index.html
```

同層需有 Cesium：

```text
out/DJB-M-SU-_/Cesium-1.141/Build/Cesium/Cesium.js
```

建議用本機 HTTP server 開，不要直接用 `file://`：

```powershell
cd C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\DJB-M-SU-_
python -m http.server 8098
```

開啟：

```text
http://127.0.0.1:8098/index.html
```

## Three.js Comparison Viewers

先準備本機 vendor，不把第三方大檔提交進 git：

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\prepare_three_vendor.ps1
```

開純 Three.js 版：

```text
http://127.0.0.1:8098/index_three.html
```

開 MapLibre + Three.js 版：

```text
http://127.0.0.1:8098/index_maplibre_three.html
```

這兩頁用來比較 renderer / 圖台整合效能，讀同一份 `tileset.json`、`tileset_smooth_90.json`、`tileset_smooth.json`。

## CAD Inspect Pipeline

Phase 1C 建立可信 inspect DB，不產生 3D Tiles：

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\run_phase1c_dxf_entity_inspect.ps1
```

主要輸出：

```text
out\inspect_tamkang\entity_inspect_report.json
out\inspect_tamkang\cad_entities\<source_id>.entities.jsonl
out\inspect_tamkang\project_inspect.db
```

目前 DWG 主線是 `ODA -> ACAD2000 / DXF -> OGR entity-level inspect`。DGN 暫時保留 source traceability，標記為 `needs_alternative_route`。

Phase 1D 從 SQLite + manifest 匯出 QA 報表，先回答為什麼 source 被 quarantine：

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\export_inspect_review.ps1
```

輸出：

```text
out\inspect_tamkang\review_report.html
```

報表內容包含 8 個 source 狀態、selected scale、raw / P0.5-P99.5 bbox、warning、quarantine reason、duplicate score、entity count、layer / geometry type 統計。

Phase 1E 做第二層 quarantine drilldown 與 approval workflow：

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\run_phase1e_drilldown.ps1
```

主要輸出：

```text
out\inspect_tamkang\qa\duplicate_pairs.json
out\inspect_tamkang\qa\entity_outliers.json
out\inspect_tamkang\qa\approved_sources.json
out\inspect_tamkang\qa\rejected_sources.json
out\inspect_tamkang\qa\needs_review_sources.json
```

Phase 1E 會同步更新 `out\inspect_tamkang\review_report.html`，加入 duplicate compare、管理中心 outlier 與 approval manifest summary。

Phase 1F 建立 approved-only publish skeleton，不做 CAD -> 3D Tiles：

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\run_phase1f_publish_skeleton.ps1
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\run_phase1f_viewer.ps1
```

主要輸出：

```text
out\inspect_tamkang\normalized\<source-id>\source_manifest.json
out\inspect_tamkang\publish\sources_manifest.json
out\inspect_tamkang\publish\debug_overlays.json
out\inspect_tamkang\publish\index.html
```

`publish\sources_manifest.json` 只包含 approved source；rejected / needs_review 只進 `debug_overlays.json` 給 QA viewer 畫 bbox。
`publish\index.html` 會內嵌這兩份 manifest，直接用 `file://` 開也不需要再 request JSON；Viewer 預設停用 Cesium ion 底圖，避免 QA skeleton 因外部底圖 request failed 中斷。
Phase 1F QA viewer 會套用 NLSC `EMAP5` WMTS 作地理參考底圖；若 WMTS 載入失敗，bbox overlay 仍可顯示。
若透過 IIS 看到 `401.3`，代表 IIS AppPool 沒有讀取 `publish` 目錄 ACL；Phase 1F QA viewer 建議直接用 `run_phase1f_viewer.ps1` 啟動 `http://127.0.0.1:8120/index.html`。

Phase 1G 在 Phase 1F publish skeleton 上加入 Spatial QA overlay，不做 geometry publish，也不做 layer isolate：

```text
out\inspect_tamkang\publish\spatial_qa_manifest.json
out\inspect_tamkang\publish\index.html
```

`spatial_qa_manifest.json` 是 browser-facing QA evidence；瀏覽器不直讀 SQLite。正式 runtime 仍只吃 `publish\sources_manifest.json` 的 approved source，rejected / needs_review / duplicate / outlier 只走 QA overlay。

Phase 1G viewer 支援：

- 點 bbox 顯示 source detail：approval、scale、bbox、warning、top layers、geometry types。
- AOI overlay：用 EPSG:3826 台灣工程 AOI 轉 WGS84 畫框。
- raw bbox / percentile bbox 切換：判讀 stray point 造成的 bbox 放大。
- duplicate compare overlay：顯示 `DJB-M-SU-監測.dwg` vs `主橋.dwg` 的重疊證據。
- outlier marker：顯示 `管理中心_全.dwg` 可疑 entity 位置，點 marker 看 FID、layer、score、reason。

Phase 1H 追加 QA navigation：

- 左側 source list / search，可依 source、status、warning、layer、geometry type 搜尋。
- source detail 顯示 `aoi_status`、`aoi_gap_m`、`bbox_inflation_ratio`。
- 點 source 直接 zoom 到 P0.5/P99.5 bbox；detail panel 可切 raw / percentile zoom。
- Duplicate / Outliers 快捷按鈕可直接打開 comparison / top outlier drilldown。

Phase 1H Runtime 建立第一條 approved-only geometry runtime skeleton，仍不是正式 CAD -> 3D Tiles：

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\run_phase1h_runtime_publish.ps1
```

主要輸出：

```text
out\inspect_tamkang\publish\runtime_manifest.json
out\inspect_tamkang\publish\runtime_budget_report.json
out\inspect_tamkang\publish\spatial_pick_index.json
out\inspect_tamkang\publish\runtime_metadata\dwg-12d5f1b6.json
out\inspect_tamkang\publish\runtime\dwg-12d5f1b6\runtime.glb
out\inspect_tamkang\publish\runtime\dwg-12d5f1b6\runtime_metadata.json
out\inspect_tamkang\publish\runtime\dwg-12d5f1b6\runtime_pick.json
```

規則：

- runtime 只讀 `qa\approved_sources.json`，目前只包含 `dwg-12d5f1b6 / 主橋塔.dwg`。
- rejected / needs_review 只留在 QA overlay，不會進 `runtime_manifest.json` 或 geometry。
- Phase 1H geometry 是 entity bbox proxy GLB，只用來驗證 approved gate、source-aware rendering、minimal metadata 與 Cesium loading。
- runtime metadata 只保留 `feature_id`、`source_id`、`explode_group_key`、`ifc_type`、`material_id`；完整 CAD/IFC 查詢仍回 inspect DB / QA manifests。
- `runtime_pick.json` 只存 bbox picking index，不是屬性資料 source of truth。
- `spatial_pick_index.json` 是 publish root 的 runtime-only pick index，先供後續 Cesium hybrid pick fallback 使用；它使用 local bbox，不修改 GLB、不塞 invisible mesh。

## Verification

```powershell
cargo test
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\verify_index_page.ps1
```

## Current Limits

- 主要支援 IFC2X3 product geometry；RVT 需先經 Revit IFC exporter。
- 幾何以 `FacetedBrep` / `ShellBasedSurfaceModel` / `MappedRepresentation` 為主。
- 顏色支援 IFC surface style，沒有 PBR material / texture。
- 不做 Draco / meshopt 壓縮。
- Viewer 目前預期輸出可包含：
  - `tileset.json`
  - `tileset_smooth_90.json`
  - `tileset_smooth.json`
