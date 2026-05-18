# IFC to Cesium 3D Tiles

Rust CLI for converting AECOsim / IFC2X3 `IfcBuildingElementProxy` + `FacetedBrep` models into Cesium 3D Tiles 1.0 (`tileset.json` + `.b3dm`).

目前目標是先把 `DJB-M-SU-_.ifc` 這類 AECOsim IFC2X3 橋梁模型穩定轉出可在 CesiumJS 載入、可點選、保留屬性、顏色正常的 3D Tiles。這不是完整通用 IFC kernel。

## Features

- STEP/IFC2X3 indexing parser, focused on AECOsim `FacetedBrep` / `MappedRepresentation`.
- Product metadata to Batch Table: IFC id、GlobalId、類型、名稱、樓層、群組、style、顏色、Pset JSON。
- IFC style color extraction with fallback report.
- EPSG:3826 to WGS84/ECEF georeferencing via `proj4rs`.
- Root ENU-to-ECEF transform with local float32 geometry.
- Spatial tiling with configurable feature / triangle limits.
- Flat and smooth normal output modes.
- Cesium demo viewer: select, focus display, explode, move-out pad, measurement, basemap switching, render scale control.

## Repository Policy

Git 只收可維護的 source / test / tool / doc：

- `src/`
- `tests/`
- `tools/`
- `Cargo.toml`
- `Cargo.lock`
- `out/DJB-M-SU-_/index.html`
- docs and license

不收大型或可再生資料：

- `*.ifc`
- `target/`
- `out/**` generated tiles, tilesets, reports
- local `Cesium-1.141` distribution

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

## Verification

```powershell
cargo test
pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\verify_index_page.ps1
```

## Current Limits

- 主要支援 AECOsim IFC2X3 `IfcBuildingElementProxy`。
- 幾何以 `FacetedBrep` / `MappedRepresentation` 為主。
- 顏色支援 IFC surface style，沒有 PBR material / texture。
- 不做 Draco / meshopt 壓縮。
- Viewer 目前預期輸出可包含：
  - `tileset.json`
  - `tileset_smooth_90.json`
  - `tileset_smooth.json`

