# history.md

## 2026-05-19

### RVT -> IFC -> GLB

- 採用 Autodesk Revit IFC exporter 路線，不使用 `dosymep/RevitCoreConsole` 當主流程。
- 新增 Rust RVT orchestration：
  - 偵測 Revit 2025 / 2026 / 2027。
  - 支援 `--revit-version auto|2025|2026|2027`。
  - RVT input 會寫 job JSON、安裝 Revit `.addin` manifest、啟動 Revit bridge、等待 IFC，再呼叫既有 IFC -> GLB / 3D Tiles pipeline。
  - 支援 `--keep-ifc`、`--bridge-assembly`、`--rvt-timeout-minutes`。
- 新增 C# `revit_bridge` add-in scaffold：
  - 讀取 `RVT_TO_GLB_JOB`。
  - 使用公開 Revit API `Application.OpenDocumentFile` 與 `Document.Export(..., IFCExportOptions)`。
  - 預設 IFC2x3 Coordination View 2.0，開啟 common property sets、Revit property sets、base quantities、material psets。
- 新增 revit-ifc tooling：
  - `tools/fetch_revit_ifc.ps1` pin `IFC_v25.4.40` / `IFC_v26.4.1` / `IFC_v27.0.1.1`。
  - `tools/build_revit_ifc.ps1` 可在本機抓回 tag 後嘗試編譯。
  - `tools/build_revit_bridge.ps1` 依 Revit 版本建置 bridge DLL。
- 擴充 IFC -> GLB：
  - 不再只限 `IfcBuildingElementProxy`，改抓有 `IfcProductDefinitionShape` 的 IFC product。
  - 保留 Revit-like property sets、surface style color、mapped representation。
  - 輸出 `<name>_flat.glb`、`<name>_smooth.glb`、`metadata.json`、`unsupported_geometry_report.json`。
  - GLB node/mesh `extras` 內含 feature metadata，另保留 `metadata.json` 方便外部檢查。
- 新增 tests：
  - Revit 版本偵測。
  - RVT job JSON 序列化。
  - Revit-like IFC wall fixture 的 flat/smooth GLB 與 metadata。
  - RVT gated integration test：需設定 `RVT_TO_GLB_SAMPLE_RVT` 且本機有 Revit 才執行，否則跳過。
- 驗證：
  - `cargo test` 通過。
  - PowerShell tooling syntax check 通過。
  - 本機未偵測到 `C:\Program Files\Autodesk\Revit 2025/2026/2027\RevitAPI.dll`，因此尚未做實機 RVT export。
- 後續補強：
  - 找不到 Revit 時，console 會提示 Autodesk Account 與 Revit Free Trial 官方入口。
  - 新增 `--revit-exe`，支援 Revit 安裝在非預設路徑時手動指定。
  - Revit 偵測不再只認 `Revit 2027` 精準資料夾，會掃描 Autodesk 安裝根目錄底下含 `Revit` 與支援年份的資料夾，例如 `Revit 2027 Release`、`Autodesk Revit 2025`。
  - Revit 2027 bridge build 修正為 multi-target `net8.0-windows;net10.0-windows`，`tools/build_revit_bridge.ps1 -Version 2027 -Configuration Debug` 已成功產生 `target/debug/revit_bridge/RvtToGlb.RevitIfcExporter.dll`。
  - 實機 Revit 2027 export 回報 `ModificationOutsideTransactionException`，已將 `Document.Export(..., IFCExportOptions)` 包入 Revit `Transaction`，失敗時 rollback。
  - Revit 開著時會鎖住 `target/debug/revit_bridge` DLL；Rust 預設 bridge 搜尋已改為優先使用 `target/revit_bridge/<version>/RvtToGlb.RevitIfcExporter.dll`，避免開發時覆蓋被鎖住的 runtime DLL。
  - 發現舊的 `*.rvt-export-result.json` 會讓 Rust 新啟動 Revit 後立刻讀到上一輪錯誤；已在每次 RVT export 前刪除 stale result，並加 regression test。

### Three.js 比較 viewer

- 新增 `out/DJB-M-SU-_/index_three.html`。
  - 純 Three.js + `3d-tiles-renderer`。
  - 用地面 / 水面 / 格網替代底圖，排除圖台變因。
- 新增 `out/DJB-M-SU-_/index_maplibre_three.html`。
  - MapLibre GL JS + Three.js custom layer + `3d-tiles-renderer`。
  - 支援 EMAP5、Google 航照圖、Google 電子地圖、OSM。
- 新增 `out/DJB-M-SU-_/three_viewer_common.js` 共用互動邏輯。
  - 選取黃色高亮。
  - 焦點顯示。
  - 爆炸。
  - 8 向移出方向盤。
  - 量距 / 量面。
  - 法線切換：平面、90°、全平滑。
  - 自動畫質：操作中 50%，停止後 100%。
  - 效能面板：FPS、frame time、tile stats、triangle estimate、memory / bytes estimate。
- 新增 `tools/prepare_three_vendor.ps1`。
  - 固定 Three.js `0.183.0`。
  - 固定 `3d-tiles-renderer` `0.4.21`。
  - 固定 MapLibre GL JS `5.24.0`。
  - vendor 放在 viewer 同層 `vendor/`，不進 git。
- 擴充 `tools/verify_index_page.ps1`，同時檢查 Cesium viewer、Three viewer、MapLibre+Three viewer 與 vendor 準備腳本。

## 2026-05-18

### 專案建立

- 建立 Rust CLI `ifc_to_3dtiles`。
- 目標鎖定 AECOsim IFC2X3 / `IfcBuildingElementProxy` / `FacetedBrep` / `MappedRepresentation`。
- 建立 STEP indexing parser、geometry pipeline、B3DM/GLB writer、tileset writer、CRS transform。
- 預設 CRS 使用 EPSG:3826，轉 WGS84/ECEF，tileset root 使用 ENU-to-ECEF transform。

### IFC Metadata / Style

- Batch Table 保留物件級 metadata：
  - `batch_id`
  - `ifc_step_id`
  - `global_id`
  - `ifc_type`
  - `name`
  - `description`
  - `dgn_element`
  - `site`
  - `building`
  - `storey`
  - `group_names`
  - `style_id`
  - `color_rgba`
  - `psets_json`
- 解析 IFC style chain，缺色使用 fallback 並寫入 `conversion_report.json`。

### Tiling / Normal Modes

- 以 feature AABB 做 spatial grid tiling。
- 支援 `--tile-max-features` 與 `--tile-max-triangles`。
- 加入平面 / 平滑法線輸出：
  - `--normal-mode flat`
  - `--normal-mode smooth`
  - `--normal-mode both`
  - `--smooth-angle-deg`
- 針對 demo 產生過三套 tiles：
  - flat
  - smooth 90°
  - smooth 180° / 全平滑
- 實測較合適的切 tile 目標約為單檔 5MB；目前因單一大型 feature 不拆分，仍可能有少數 tile 超過 5MB。

### Cesium Viewer

- 在 `out/DJB-M-SU-_/index.html` 建立 demo viewer。
- Cesium 改用同層相對路徑 `./Cesium-1.141/Build/Cesium/`。
- 加入工具列：
  - 選取
  - 焦點顯示
  - 透地
  - 量距
  - 量面
  - 底圖選擇
  - 法線切換
  - 畫質控制
  - 爆炸
  - 移出方向盤
- 底圖支援：
  - EMAP5
  - Google 航照圖
  - Google 電子地圖
  - OSM
- 操作手感調整成較接近 ArcGIS，降低亂飛感。

### Selection / Focus / Explode / Move-Out

- 焦點 OFF 時仍可選取物件、看 metadata、爆炸、移出。
- 焦點 ON 時：
  - 被點選物件維持實體顯示。
  - 其他物件變半透明線稿。
- 爆炸只作用在被選取物件。
- 移出只作用在被選取物件。
- 移出 UI 改成 8 向方向盤，中間 `0` 還原，預設西南。
- 切換法線模式時，已選取物件會保持移出 / 爆炸狀態。

### Performance

- 加入 `viewer.resolutionScale` 畫質滑桿。
- 加入 `自動` 勾選：
  - 圖台移動中暫時降到 50%。
  - 停止後回到 100%。

### Git 化

- 加入 `.gitignore`。
- 決定 source / tests / tools / docs / viewer shell 進 git。
- 忽略 IFC、`target/`、generated tiles、local Cesium package。

### Revit IFCFACEBASEDSURFACEMODEL

- 使用 Revit 2027 匯出的 `阻尼器(比例需調整).ifc` 已確認是 `IFC2X3`，Body representation 為 `IFCFACEBASEDSURFACEMODEL((#IFCCONNECTEDFACESET))`。
- 原本 IFC→GLB 只支援 `IFCFACETEDBREP`、`IFCSHELLBASEDSURFACEMODEL`、`IFCMAPPEDITEM`，因此 RVT→IFC 成功後仍回報「沒有可轉換的 IFC product 幾何」。
- 新增 `mesh_face_based_surface_model`，沿用既有 `mesh_shell` 展開 `IFCFACE` / `IFCPOLYLOOP` 並產生 triangles。
- `convert.rs` 新增 `IFCFACEBASEDSURFACEMODEL` match arm，套用既有 transform、顏色 fallback、missing-color 統計流程。
- 驗證：
  - `cargo test` 通過。
  - `cargo build` 通過。
  - 直接轉 `out/阻泥器/阻尼器(比例需調整).ifc` 到 `out/阻泥器_ifc_verify/阻尼器(比例需調整)` 成功，產出 flat/smooth GLB、metadata、tiles、unsupported report。

### Local Project Baseline Freeze

- 新增 Task 0：先做現場盤點 / Freeze Baseline，再開始 project ingest 實作。
- `git status --short --branch`：`main...origin/main`，原本只剩 `sample_files/` 未追蹤；已補 `.gitignore` 忽略 `sample_files/`、`*.dgn`、`*.dwg`、`*.dxf`。
- `git log --oneline -5` 最新為：
  - `f01bcb5 Add CAD inspect metadata priorities to ingest plan`
  - `678ae86 Clarify per-source tileset publish plan`
  - `536c0fa Plan local project ingest workflow`
  - `c923d19 擴充 rvt`
  - `986cb16 擴充 rvt`
- `cargo test`：目前 shell PATH 找不到 `cargo`，Task 1 前需先修 Rust toolchain / PATH。
- 新增 `tools/inspect_cad_sources.ps1`，用於盤點 CAD 工具與樣本檔案分布，不執行轉檔。
- CAD probe 結果：
  - `ogrinfo`: `C:\ms4w_MSSQL\GDAL\ogrinfo.exe`
  - `ogr2ogr`: `C:\ms4w_MSSQL\GDAL\ogr2ogr.exe`
  - `ODAFileConverter`: `C:\bin\ODAFileConverter\ODAFileConverter.exe`
  - ODA File Converter 版本：`20.12.0.0`，標記為 `too_old_for_2026_cad_delivery`
  - sample files：8 檔，CAD 7 檔
  - `.dwg`: 4 檔，149,951,738 bytes
  - `.dgn`: 3 檔，240,211,456 bytes
  - `.ifc`: 1 檔，117,439,099 bytes
- 架構分層確認：
  - Rust = 主控與可信 pipeline
  - 外部工具 = CAD/DGN/DWG 轉換
  - SQLiteDB = 屬性資料
  - Cesium/JS = 顯示與互動
- 產品方向確認為 Local Web Platform / BIM-GIS workstation：
  - 後端核心優先 Rust CLI / worker，後續可加 Axum local API。
  - 可銜接既有 3wa 風格 PHP Dashboard 讀 SQLite / manifest。
  - 前端優先 Bootstrap + jQuery + GoldenLayout + Tabulator + jsTree + Cesium。
  - Cesium 是正式 GIS/BIM viewer；Three.js 只作 GLB / small model debug viewer。
  - 第一版不做 Rust GUI、Qt Desktop、Electron 全包。
