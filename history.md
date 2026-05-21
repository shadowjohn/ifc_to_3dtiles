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
- 舊版實測較合適的切 tile 目標約為單檔 5MB；後續金門大橋案例發現單一大型 feature 會形成 400MB b3dm，已改為更小預設與單一 feature triangle chunk 切分。

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

### ODA File Converter Side-By-Side Update

- 使用者已安裝新版 ODA File Converter：
  - `C:\Program Files\ODA\ODAFileConverter 27.1.0\ODAFileConverter.exe`
  - `ProductVersion`: `27.1.0.0`
- 舊版保留：
  - `C:\bin\ODAFileConverter\ODAFileConverter.exe`
  - `ProductVersion`: `20.12.0.0`
- `tools/inspect_cad_sources.ps1` 已改成掃描多個 ODA 版本：
  - `C:\bin\ODAFileConverter*`
  - `C:\Program Files\ODA\ODAFileConverter*`
  - `ODA_FILE_CONVERTER_EXE` 環境變數覆寫
- CAD probe 現在輸出：
  - `oda_file_converters`
  - `preferred_oda_file_converter`
  - 相容舊欄位 `tools.oda_file_converter`
- 驗證結果：
  - preferred ODA 指向 `27.1.0.0`
  - `27.1.0.0` 標記為 `acceptable_baseline`
  - `20.12.0.0` 仍列出並標記為 `too_old_for_2026_cad_delivery`

### Project Ingest Task 1-3

- 新增 local project workspace manifest：
  - `WorkspaceLayout`
  - `ProjectManifest`
  - `SourceRecord`
  - `SourceFormat`
  - `SourceStatus`
- 新增 source discovery inspect 基礎：
  - 掃描 `.ifc`、`.rvt`、`.dgn`、`.dwg`
  - 略過未知副檔名
  - 產生 ASCII stable source id
  - CLI 新增 `inspect --input --output --source-epsg`
- 新增 georef / scale classifier：
  - 支援 scale candidates：`1000.0 / 1.0 / 0.1 / 0.01 / 0.001`
  - 使用 centroid + percentile bounds 判斷 AOI
  - raw bbox 飛出 AOI 時記 warning，不直接否決有效 source
  - `SourceTransform` 固定 canonical space：`EPSG:3826 meters / local ENU / Z-up`
- 驗證：
  - `cargo test` 通過
  - `ifc_to_3dtiles inspect` 跑 `sample_files\淡江大橋移交模型` 成功
  - `source_manifest.json` 抓到 8 個 source：DGN 3、DWG 4、IFC 1

### CAD Metadata Inspect Dump

- 新增 CAD hierarchy dump schema：
  - `models`
  - `references`
  - `levels`
  - `cells`
  - `shared_cells`
  - `attachments`
  - `element_classes`
  - `materials`
  - `line_styles`
  - `warnings`
- `inspect` 現在會為 DGN/DWG source 產生 `cad_metadata/<source-id>.json`。
- 尚未做真實 ODA/GDAL hierarchy 解析前，先寫 empty buckets 並記 warning，確保 viewer / publish / debug 有固定資料入口。
- 修正 source id 碰撞：
  - 原本中文檔名如 `主橋.dwg`、`主橋塔.dwg` 會壓成相同 ASCII slug。
  - 現在 source id 使用 ASCII slug + deterministic path hash。
- `inspect` 重跑時會清掉舊 `cad_metadata/*.json`，避免 stale sidecar 汙染報告。
- 驗證：
  - `source_manifest.json` source id 8/8 唯一。
  - CAD source 7 個，sidecar 7 個。

### Quarantine / Duplicate Core

- 新增 source status decision 核心：
  - centroid + percentile bounds 判斷 scale / AOI。
  - Z range 小於 5cm 時標記為 2D 並 quarantine。
  - 所有 scale candidates 都不在 AOI 時 quarantine。
- 新增 geometry fingerprint duplicate scoring：
  - vertex count
  - triangle count
  - bbox
  - surface area
- 這階段先提供可測核心，不直接套到 DGN/DWG inspect manifest；原因是 DGN/DWG 尚未有可信 geometry bbox / triangle / vertex 統計。

### CAD Probe Summary Integration

- Rust `inspect` 模組新增可解析 `cad_probe_report.json` 的 schema。
- 支援欄位：
  - `tools.ogrinfo`
  - `tools.ogr2ogr`
  - `tools.oda_file_converter`
  - `oda_file_converters`
  - `preferred_oda_file_converter`
  - `extension_distribution`
  - `cad_files`
- 驗證目前 probe report：
  - preferred ODA 為 `27.1.0.0`
  - 舊 ODA `20.12.0.0` 仍可列入 fallback
  - sample files 分布維持 `.dwg` 4、`.dgn` 3、`.ifc` 1
- 直接測試 `ogrinfo` 樣本：
  - DGN 樣本目前 GDAL 無法開啟。
  - DWG 樣本回報 libopencad 只支援 DWG R2000，無法支援目前 DWG 版本。
  - 因此短期 DGN/DWG hierarchy 不能依賴 GDAL/OGR，仍需 ODA conversion/probe route。

### Phase 1A Inspect Foundation Completed

- 今日成果定義為 `Phase 1A: Inspect foundation completed`。
- 已證實 OGR 不能當這批 DGN/DWG 的主路線：
  - DGN 樣本無法由目前 GDAL 開啟。
  - DWG 樣本版本超出 libopencad 支援範圍。
- 下一階段方向不再卡 OGR 原始檔 inspect，改走：
  - ODA 27.1.0 baseline
  - ODA conversion smoke
  - DGN/DWG 轉出 normalized CAD
  - 轉完再做 bbox / scale / fingerprint / hierarchy inspect
- 明日計畫已寫入：
  - `docs/superpowers/plans/2026-05-20-phase-1b-oda-conversion-smoke.md`

### Phase 1B Plan補充

- 在 ODA conversion smoke 前新增 `Task 0: Source Manifest Usability Fields`：
  - `display_name`
  - `original_file_name`
  - `relative_path`
- 針對 `DJB-M-SU-監測.dgn.i.dgn` 這類重複 CAD-like 副檔名新增 warning：
  - `possible_intermediate_or_export_copy`
- ODA conversion report 也要帶出：
  - `source_display_name`
  - `source_original_file_name`
  - `source_relative_path`

### Phase 1B ODA Conversion Smoke

- 已實作並執行 `tools/convert_cad_with_oda.ps1`。
- 本次全量 smoke：
  - attempted：7
  - success：4
  - failed：3
  - ODA version：`27.1.0.0`
  - target：`ACAD2018 / DWG`
- 結果判讀：
  - 4 筆 `.dwg` 可由 ODA 轉出 normalized DWG。
  - 3 筆 `.dgn` 皆失敗，ODA `.err` 顯示 `Invalid group code`，代表不能把這批 DGN 直接丟 ODA File Converter 當 DWG 類檔處理。
  - `*.dgn.i.dgn` 已帶出 `possible_intermediate_or_export_copy` warning，後續 duplicate / overlap 判斷要特別看。

### Phase 1B Normalized CAD Re-Inspect

- 已新增 `tools/inspect_normalized_cad.ps1`。
- `ACAD2018 / DWG`：
  - ODA 可轉出 4 筆 DWG。
  - 目前 GDAL/libopencad 仍無法 inspect，錯誤顯示只支援 `DWG R2000 [ACAD1015]`。
- `ACAD2000 / DWG`：
  - 檔頭確認為 `AC1015`。
  - GDAL/libopencad 仍失敗，錯誤為 `HEADERVARS section CRC doesn't match`。
- 單筆 `ACAD2000 / DXF` 探針：
  - `DJB-M-SU-監測.dwg` 可轉出 DXF 並被 OGR inspect。
  - OGR 可取得 layer extent，但 raw bbox 很大且 scale candidate 為空。
  - 下一步應走 `ODA -> DXF -> entity-level/percentile bbox`，不能只信 DXF layer extent。

### Phase 1C DXF Entity-Level Inspect

- 已新增 Rust `cad_entity_inspect` core：
  - 解析 OGR `ogrinfo -geom=YES` 的 WKT geometry。
  - 支援 `POLYHEDRALSURFACE Z` 座標掃描。
  - 產生 entity bbox、source raw bbox、P0.5/P99.5 percentile bbox、scale classifier、fingerprint。
- 已新增 `tools/run_phase1c_dxf_entity_inspect.ps1`：
  - source inspect
  - CAD probe
  - DWG only `ODA -> ACAD2000 / DXF`
  - layer-level evidence report
  - entity-level inspect + SQLite
- 本次實測：
  - DWG -> DXF：4/4 成功。
  - entity count：52,530。
  - parsed entity count：52,530。
  - skipped entity count：0。
  - `project_inspect.db` tables：`sources` 8、`entities` 52,530、`entity_bboxes` 52,530、`source_stats` 4、`fingerprints` 4、`conversion_runs` 4。
  - 3 筆 DGN 已回填 `needs_alternative_route`，原因是 `ODA invalid group code`。
- 初步分類：
  - `主橋塔.dwg`：`approved`，`selected_scale = 1.0`。
  - `DJB-M-SU-監測.dwg`、`主橋.dwg`、`管理中心_全.dwg`：`quarantined`，因 percentile bbox 仍超出 EPSG:3826 AOI，需下一步做 entity/layer outlier analysis。

### Phase 1D Inspect Review Dashboard / QA Report

- 新增 Rust `inspect_review` core：
  - 從 `project_inspect.db` + `source_manifest.json` 產生 review model。
  - 匯出 self-contained `review_report.html`，不吃 CDN，不碰 3D Tiles。
  - source list 顯示 8 個 source 的 status、selected scale、entity count、P0.5/P99.5 bbox、warning count、duplicate summary。
  - source detail 顯示 raw bbox、percentile bbox、z range、quarantine reason、warnings、top layers、geometry types。
- 新增 duplicate QA scoring：
  - 使用 percentile bbox、entity count、vertex count、layer histogram overlap。
  - 分數大於 0.8 時列入 duplicate candidates，供 publish 前人工確認。
- 新增 quarantine reason 判讀：
  - scale classifier 無法選出 allowed scale。
  - P0.5/P99.5 bbox 超出 AOI。
  - raw bbox 與 percentile bbox 差距過大，疑似 stray point / construction line。
  - z range 明確標示「不是 2D」或「可能是 2D」。
  - DGN 固定標示 `needs_alternative_route / ODA invalid group code`。
- 新增 `tools/export_inspect_review.ps1` 一鍵匯出：
  - 預設輸入 `out\inspect_tamkang`
  - 預設輸出 `out\inspect_tamkang\review_report.html`

### Phase 1E Quarantine Drilldown + Approval Workflow

- 新增 Rust `inspect_drilldown` core：
  - 直接讀 `project_inspect.db` + `source_manifest.json`。
  - 不重跑 ODA / OGR，不轉 3D Tiles。
  - 產生第二層 QA artifacts 到 `out\inspect_tamkang\qa`。
- Duplicate pair compare：
  - 比較 `DJB-M-SU-監測.dwg` / `主橋.dwg` 的 bbox、entity count、vertex count、layer histogram、geometry type histogram、fingerprint。
  - 目前建議保留 `DJB-M-SU-監測.dwg`，`主橋.dwg` 先列 rejected duplicate candidate。
- 管理中心 outlier drilldown：
  - 針對 `管理中心_全.dwg` 產出 `entity_outliers.json`。
  - 列出最遠 entity、最大 bbox entity、最大 Z range entity、outside AOI entity 與 layer outlier summary。
- Approval manifests：
  - `approved_sources.json`
  - `rejected_sources.json`
  - `needs_review_sources.json`
  - 第一版是機器建議 + 人工可修正，後續 publish pipeline 再把這三份當正式 gate。
- `review_report.html` 現在會加入 Phase 1E QA summary。

### Phase 1F Approved Source Publish Skeleton

- 新增 Rust `publish_skeleton` core：
  - 只讀 `qa/approved_sources.json` 作正式 publish runtime source。
  - `qa/rejected_sources.json` 與 `qa/needs_review_sources.json` 只輸出到 debug overlay。
  - 不做 CAD -> 3D Tiles，也不複製大型 DXF。
- 新增 CLI：
  - `ifc_to_3dtiles publish-approved --input out\inspect_tamkang --output out\inspect_tamkang\publish`
- 新增 `tools/run_phase1f_publish_skeleton.ps1`。
- 輸出：
  - `normalized/<source-id>/source_manifest.json`
  - `publish/sources_manifest.json`
  - `publish/debug_overlays.json`
  - `publish/index.html`
- 目前 approved runtime source 只包含：
  - `dwg-12d5f1b6 / 主橋塔.dwg`
- `review_report.html` 會追加 Phase 1F publish skeleton summary。
- 修正 `publish/index.html` 在 `file://` 直接開啟時的 QA viewer 問題：
  - 內嵌 `sources_manifest.json` / `debug_overlays.json`，避免 file mode request JSON 失敗。
  - 停用 Cesium 預設 ion/network 底圖，避免 skeleton viewer 因外部 request failed 觸發 render loop error。
- 新增 `tools/run_phase1f_viewer.ps1`：
  - 用 Python 本機 server 開 `publish/index.html`。
  - 避免 IIS / ASP.NET 因 AppPool ACL 造成 `401.3`。
- Phase 1F QA viewer 加入 NLSC `EMAP5` WMTS 作地理參考底圖，讓 bbox overlay 可直接對台灣地圖位置。

### Phase 1G Spatial QA Interaction

- 新增 Rust `spatial_qa` core：
  - 產生 browser-facing `publish/spatial_qa_manifest.json`。
  - 來源是 `source_manifest.json`、`project_inspect.db`、Phase 1E QA manifests、duplicate pairs 與 outlier report。
  - browser 不直讀 SQLite，正式 runtime 仍只吃 approved `sources_manifest.json`。
- Phase 1G-A：
  - `spatial_qa_manifest.json` 現在保留 approved / debug source 分流、AOI、duplicate compare、outlier marker、bbox、warnings、top layers、geometry type stats。
  - rejected / needs_review / duplicate / outlier 全部只走 Spatial QA overlay，不會進正式 publish runtime。
- Phase 1G-B：
  - `publish/index.html` 加入右側 source detail panel。
  - 支援點 bbox 顯示 approval reason、selected scale、raw / percentile bbox、warnings、top layers、geometry types。
  - 支援 AOI overlay、raw bbox / percentile bbox 切換、duplicate compare overlay、outlier marker。
  - 點 outlier marker 可看到 FID、layer、handle、geometry type、score、reason、bbox。
- 目前 QA manifest 統計：
  - approved runtime source：1
  - debug source：7
  - duplicate pair：1
  - outlier marker：40
- 本階段仍不做 geometry publish，也不做 layer isolate。

### Phase 1H Spatial QA Navigation

- 在 `spatial_qa_manifest.json` 的 source detail 補：
  - `aoi_status`
  - `aoi_gap_m`
  - `bbox_inflation_ratio`
- `aoi_gap_m` 以 EPSG:3826 meter 表示 W/S/E/N 四方向超出 AOI 的距離，方便快速判斷 quarantine 是否為飛點 / scale / origin 問題。
- `bbox_inflation_ratio` 用 raw bbox 面積除以 percentile bbox 面積，快速看 raw bbox 是否被 stray geometry 放大。
- `publish/index.html` 補左側 `Source QA` navigation：
  - source list
  - search
  - source click zoom
  - duplicate drilldown
  - top outlier list
- 右側 detail panel 現在能直接 zoom raw / percentile bbox。
- 本階段仍不做 geometry publish，也不做 layer isolate；正式 runtime gate 仍只吃 approved。

### Phase 1H Approved Geometry Runtime Skeleton

- 新增第一條 approved-only runtime geometry skeleton：
  - CLI：`ifc_to_3dtiles runtime-publish --input out\inspect_tamkang --output out\inspect_tamkang\publish`
  - wrapper：`tools/run_phase1h_runtime_publish.ps1`
- Runtime 只讀 `qa/approved_sources.json`：
  - 目前 runtime source 只包含 `dwg-12d5f1b6 / 主橋塔.dwg`
  - rejected / needs_review 不會進 `runtime_manifest.json`、`runtime_budget_report.json` 或 runtime geometry。
- 新增 Rust modules：
  - `runtime_publish.rs`
  - `runtime_metadata.rs`
  - `runtime_geometry.rs`
- 輸出：
  - `publish/runtime_manifest.json`
  - `publish/runtime_budget_report.json`
  - `publish/runtime_metadata/<source-id>.json`
  - `publish/runtime/<source-id>/runtime.glb`
  - `publish/runtime/<source-id>/runtime_metadata.json`
  - `publish/runtime/<source-id>/runtime_pick.json`
- Geometry 採 Phase 1H 指定的 `Entity BBox Proxy`：
  - 每個 approved entity bbox 產生一個 proxy box。
  - 退化 bbox 會補 0.25m 最小厚度。
  - GLB 使用 source percentile bbox center 當 local origin，並在 `runtime_manifest.json` 寫入 `model_matrix`、`origin_epsg3826`、`origin_wgs84`。
  - GLB 寫 `_BATCHID`，batch order 對應 `runtime_metadata.features`。
- Runtime metadata 嚴格保持輕量：
  - `feature_id`
  - `source_id`
  - `explode_group_key`
  - `ifc_type`
  - `material_id`
  - 禁止 `psets_json`、CAD hierarchy、raw property dump、大型文字欄位。
- `runtime_pick.json` 只作 Cesium picking index，不是完整屬性 source of truth。
- `publish/index.html` 新增 `approved geometry` toggle：
  - 讀 `runtime_manifest.json`
  - 載 `runtime.glb`
  - 讀 `runtime_pick.json` 建透明 pick boxes
  - 點 runtime feature 顯示 minimal metadata
  - bbox / AOI / duplicate / outlier QA overlays 繼續可用。

### Phase 1G-C Runtime Pick Index Backend

- 新增 backend-only `publish/spatial_pick_index.json`：
  - `version`
  - `crs`
  - `sources`
  - `features`
  - `warnings`
- 每個 feature 保留 runtime pick fallback 需要的輕量欄位：
  - `featureId`
  - `sourceId`
  - `layer`
  - `name`
  - `category`
  - `bbox`
  - `center`
  - `radius`
  - `metadataRef`
- `spatial_pick_index.json` 使用 `local` bbox，搭配每個 source 的 `origin_epsg3826`、`origin_wgs84`、`model_matrix`，為後續 Cesium ray-vs-bbox fallback 做準備。
- 若 feature 缺 bbox，會寫入 `warnings` 並跳過該 feature，不中斷 publish。
- `runtime_budget_report.json` 補：
  - `pick_index_generated`
  - `pick_index_feature_count`
  - `pick_index_warnings`
- 本階段不改 viewer click 行為、不改 GLB geometry、不塞 invisible mesh。

### Phase 1J Source QA Decision Workflow

- `publish/index.html` 新增 runtime-only source QA decision workflow：
  - `Approve`
  - `Reject`
  - `Needs Review`
  - `Alternative Route`
  - reviewer note
  - browser download `source_qa_decisions.json`
- 決策結果不回寫 `qa/approved_sources.json`、`publish/sources_manifest.json` 或 geometry publish gate。
- `tools/verify_index_page.ps1` 的 `runtime_qa_report.json` 補決策統計：
  - `approvedCount`
  - `rejectedCount`
  - `needsReviewCount`
  - `alternativeRouteCount`
- DGN / alternative route source 在 viewer 預設歸為 `Alternative Route`，避免和一般人工複查混在一起。

### Phase 1K Minimal Geometry Preview

- 新增 approved-only 最小幾何 preview pipeline：
  - CLI：`ifc_to_3dtiles geometry-preview --input out\inspect_tamkang --output out\inspect_tamkang\publish`
  - wrapper：`tools/run_phase1k_geometry_preview.ps1`
- 新增 Rust `geometry_preview` core：
  - 只讀 `qa/approved_sources.json` 進入 preview。
  - 使用 `project_inspect.db` 的 entity bbox / geometry type。
  - `LINESTRING` / polyline 類型產生細 bbox prism。
  - `POLYGON` / `POLYHEDRALSURFACE` / face 類型產生簡化 bbox volume mesh。
  - 不做材質 fidelity、不做 LOD、不做壓縮、不塞完整 BIM/CAD metadata。
- 輸出：
  - `publish/geometry_preview/raw.glb`
  - `publish/geometry_preview/tile.glb`
  - `publish/geometry_preview/tileset.json`
  - `publish/geometry_preview/geometry_publish_report.json`
- 本次 approved source `dwg-12d5f1b6 / 主橋塔.dwg` preview 統計：
  - feature：1,314
  - line feature：35
  - surface feature：1,279
  - triangle：15,768
  - raw/tile GLB：約 1.9MB
- `publish/index.html` 新增預設開啟的 `minimal geometry preview` toggle：
  - 載入 `geometry_preview/raw.glb`
  - QA bbox / AOI / rejected / needs review / duplicate / outlier overlay 繼續可開關。
  - `spatial_pick_index.json` 與 hybrid pick flow 不變。

### Phase 1K-Fix Bad Geometry Triage

- 新增 bad geometry diagnostic report，不急著修模型，先分類幾何問題來源：
  - 座標 / transform 可疑
  - face / degenerate triangle
  - line / polyline 太細或零厚度
  - source outlier
  - geometry bbox 與 pick bbox mismatch
- `geometry-preview` 會同步輸出：
  - `publish/geometry_preview/geometry_diagnostic_report.json`
- 每個 published preview feature 記錄：
  - `vertexCount`
  - `triangleCount`
  - `bbox`
  - `pickBBox`
  - `center`
  - `size`
  - `hasNaN`
  - `hasDegenerateTriangles`
  - `normalStatus`
  - `transformStatus`
  - `bboxCenterDistance`
  - `bboxSizeRatio`
  - `problemCategory`
- `publish/index.html` 新增診斷 overlay：
  - `bad geometry only`
  - `bbox mismatch`
  - `outlier geometry`
- 本階段只做驗屍與可視化，不修改正式 publish schema、不修 geometry、不改 spatial pick schema。

### Phase 1L Geometry Diagnostics

- 將 1K-Fix 擴成正式 geometry diagnostics pipeline，root publish 也會輸出：
  - `publish/geometry_diagnostic_report.json`
  - 舊路徑 `publish/geometry_preview/geometry_diagnostic_report.json` 保留相容。
- 每個 feature 診斷欄位補強：
  - `diagonalLength`
  - `hasInfinite`
  - `degenerateTriangleCount`
  - `zeroAreaTriangleCount`
  - `duplicateVertexRatio`
  - `bboxOverlapRatio`
  - `mismatchLevel`
  - `distanceFromSceneCenter`
  - `sizePercentile`
  - `triangleDensity`
  - `abnormalAspectRatio`
  - `severityScore`
  - `problemFlags`
- Viewer diagnostics overlay 補：
  - severity heat color
  - geometry bbox vs pick bbox compare：geometry bbox 依 severity 上色，pick bbox 用 cyan。
  - filters：NaN、huge bbox、tiny bbox、degenerate、transform mismatch。
- `runtime_qa_report.json` 補 geometry diagnostics summary：
  - `badGeometryCount`
  - `bboxMismatchCount`
  - `NaNGeometryCount`
  - `outlierGeometryCount`
- 本階段仍只定位問題，不修 geometry、不改 publish schema。

### Phase 1M Geometry Transform Diagnosis

- 新增 transform diff report，先診斷、不修 geometry：
  - `publish/geometry_transform_diff_report.json`
- 只納入 Phase 1L 標出的 bbox mismatch / transform mismatch / far away feature。
- 每筆 diff 保留：
  - `geometryBBox`
  - `pickBBox`
  - `geometryCenter`
  - `pickCenter`
  - `centerDelta`
  - `centerDistance`
  - `geometrySize`
  - `pickSize`
  - `sizeRatioXYZ`
  - `diagonalRatio`
  - `overlapRatio`
  - `possibleCause`
- `possibleCause` 目前分類：
  - `local_world_offset`
  - `axis_swap`
  - `sign_flip`
  - `scale_mismatch`
  - `z_offset`
  - `source_offset_missing`
  - `tiny_bbox_noise`
  - `unknown`
- far away feature 會額外輸出：
  - `distanceFromSceneCenter`
  - `nearestNormalFeatureDistance`
  - `sourceOffsetCandidate`
- `runtime_qa_report.json` 補：
  - `transformDiffFeatureCount`
  - `possibleCauseHistogram`
  - `farAwayFeatureIds`
- 本階段不修 tiny bbox、不改 viewer UI。

### Phase 1N Degenerate Geometry Cleanup

- minimal geometry preview 對 tiny / zero-area / near-zero feature 加入 cleanup classification：
  - `skip`
  - `keep_as_point_marker`
  - `inflate_for_debug_only`
  - `keep_raw`
- preview publish safety：
  - 過短 line / layer `0` noise 不輸出 mesh。
  - tiny surface 但仍有 layer metadata 者，改輸出 debug marker。
  - 長線但 bbox 近零者，只做 debug-only inflation，不再當作 transform mismatch。
- `geometry_publish_report.json` 補：
  - `skipped_tiny_feature_count`
  - `debug_marker_count`
  - `degenerate_skipped_count`
  - `debug_inflated_feature_count`
- `geometry_diagnostic_report.json` 每個 feature 補：
  - `cleanupAction`
  - `meshExported`
- Phase 1N 的重點是降低 preview 診斷噪音；正式 schema、spatial pick schema、viewer UI 皆不改。

### Phase 2A Visual Geometry Quality

- 採用 C 版：Visual + QA baseline。
- minimal geometry preview 仍維持單一 GLB，不先拆 surfaces / lines / markers 多包。
- Rust preview 端新增 rule-based visual category：
  - `wall`
  - `slab`
  - `beam`
  - `column`
  - `annotation`
  - `linework`
  - `marker`
  - `unknown`
- `geometry_publish_report.json` 補：
  - `visual_category_counts`
  - `line_width_exaggeration`
  - `surface_shading_mode`
  - `double_side_debug_available`
- viewer 新增 Visual Preview 控制與 stats：
  - surfaces / lines / markers
  - QA bbox
  - pick overlay
  - double-side debug
  - triangle / line / skipped / marker / inflated / category histogram
- 新增 screenshot baseline 工具：
  - `tools/run_phase2a_preview_screenshot.ps1`
  - `tools/phase2a_preview_screenshot.mjs`
  - 輸出 `publish/screenshots/phase2a_preview.png`
  - 輸出 `publish/screenshots/phase2a_visual_report.json`
- 本階段先產可重跑的視覺基準，不做 pixel diff，不修正式 CAD geometry。

### Phase 2B Semantic Geometry Classification

- 目標：降低 approved preview 的 suggested unknown ratio，讓 preview 開始具備基本 BIM/CAD semantic。
- 採用 dual-track classification：
  - `strictCategory` / `strictConfidence`：正式統計、後續 gate / QA report 使用。
  - `suggestedCategory` / `suggestedConfidence`：viewer coloring、semantic legend、人工判讀輔助使用。
  - `matchedRuleId` / `inferenceReason`：保留命中的 config 規則與分類來源。
- 語意規則改為外部 config：
  - 預設規則：`config/semantic_rules.default.json`
  - 個案覆寫：`config/semantic_rules.local.json`
  - `.gitignore` 忽略 local 規則，避免把個案 layer/source 規則寫回核心。
  - 規則支援 category、strict / suggested keywords、layer / source regex、geometry type condition、confidence、bbox aspect hint。
- 分類類別固定：
  - `wall`
  - `slab`
  - `beam`
  - `column`
  - `pipe`
  - `annotation`
  - `terrain`
  - `linework`
  - `marker`
  - `unknown`
- `geometry_publish_report.json` 補：
  - `strict_category_counts`
  - `suggested_category_counts`
  - `strict_semantic_coverage`
  - `suggested_semantic_coverage`
  - `strict_unknown_ratio`
  - `suggested_unknown_ratio`
  - `category_confidence_histogram`
  - `semanticRulesSource`
  - `semanticRulesVersion`
- viewer 補 semantic legend / category filter / strict-vs-suggested mode；GLB 顏色使用 `suggestedCategory`。
- 本階段不改 source approval gate、不改 geometry transform、不改 spatial pick / diagnostics schema。

### Phase 2B Guardrail

- 新增 `tools/check_semantic_guardrail.ps1`。
- `tools/verify_index_page.ps1` 會先執行 guardrail。
- guardrail 掃描 `src/**/*.rs`，禁止 production Rust 出現個案 source/layer 關鍵字。
- 個案語意允許留在 tests、docs、或本機 `config/semantic_rules.local.json`。
- 移除 production 既有個案詞：
  - duplicate recommendation 改為 generic entity-count 策略。
  - QA outlier drilldown 改為挑選 raw bbox diagonal 最大的 source。
  - runtime group 顏色改成 hash-based，不看特定 layer 名稱。

### RVT Bridge Install Dir Auto Hint

- 使用者執行 `--bridge-assembly "C:\Program Files\Autodesk\Revit 2027"` 時，原本會被當成 bridge DLL 路徑而失敗。
- `--bridge-assembly` 現在若收到含 `Revit.exe` 的 Revit 安裝資料夾，會自動把它當成 Revit install hint 使用，支援 2025 / 2026 / 2027。
- bridge DLL 仍使用本工具預設位置 `target\revit_bridge\<version>\RvtToGlb.RevitIfcExporter.dll`，或可直接傳 DLL / 含 DLL 的資料夾。
- 本機已用 Revit 2027 release bridge 實測 RVT -> IFC -> GLB 成功。
- 目前 checkout 曾缺 `tileset_smooth_90.json` alias；已補回 converter 與 regression test。

### B3DM Tile Size Control

- 金門大橋輸出發現 `tile_0054.b3dm` 約 400MB；根因是 tiler 只在 feature 之間切，單一 IFC product 超過 triangle budget 時仍整包塞進一顆 b3dm。
- 新增單一大 feature triangle chunk 切分：同一 feature 會保留原始 triangles，只分散到多顆 b3dm，不做 decimation、不降畫質。
- 預設 tiling 從 `tile_max_features=500` / `tile_max_triangles=200000` 改為 `50` / `20000`，目標讓一般 b3dm 約落在 2-3MB 級距。
- 新增 regression test：單一 12 triangle feature 在 `tile_max_triangles=4` 時會拆成 3 顆 b3dm，smooth tile 同步拆分。
- 已用金門大橋 IFC 重轉驗證：`tiles` / `tiles_smooth` / `tiles_smooth_90` 各 435 顆 b3dm，最大單檔約 2.41MB、平均約 2.22MB。

### IFC SweptSolid Cable Recovery

- 使用者指出 `CJ02-金門大橋_F03_20260521.ifc` 轉出後鋼索不見。
- 調查結果：鋼索存在於 IFC，product 為 `#6603771 IFCBUILDINGELEMENTPROXY`，名稱 `P44橋柱預力鋼纜:P44橋柱預力鋼纜:531572`。
- 幾何路徑是 `MappedRepresentation -> SweptSolid -> IFCEXTRUDEDAREASOLID`，profile 為 `IFCCIRCLEPROFILEDEF`，不是 `IfcCableSegment` / `IfcTendon` / `IfcSweptDiskSolid`。
- 根因：converter 原本只支援 Brep / FaceBasedSurfaceModel / ShellBasedSurfaceModel / MappedItem，沒有把 `IFCEXTRUDEDAREASOLID + IFCCIRCLEPROFILEDEF` 建成 mesh；空 mesh skipped product 又未被寫入 unsupported report。
- 已新增圓形 profile extrusion meshing，預設用 24 segment 轉成封閉圓柱，不做減面。
- 已補 regression tests：
  - `swept_solid_circle_proxy_converts_to_glb_with_metadata`
  - `unsupported_report_includes_skipped_empty_products`
- 實測重轉 `CJ02-金門大橋_F03_20260521.ifc`：converted features 從 87 增為 88，metadata 已包含 `P44橋柱預力鋼纜`；tile 最大約 2.41MB。
- 剩餘未支援項目目前會正確列在 `unsupported_geometry_report.json`：`IFCEXTRUDEDAREASOLID` 65 個多為 arbitrary profile with voids，`IFCGEOMETRICCURVESET` 2 個。

### HTML Template Basemap Selector

- 使用者要求 `out/html_template` 範本加入常用底圖選擇：EMAP5、Google 街景圖、Google 航照圖、OSM。
- `index.html` Cesium 版已有底圖 selector 與 provider；本次統一顯示文字與排序。
- `index_maplibre_three.html` / `index_three.html` 補可見「底圖」標籤，選項統一為 `EMAP5`、`Google 街景圖`、`Google 航照圖`、`OSM`。
- `three_viewer_common.js` 的 MapLibre raster basemap label 同步改為 `Google 街景圖`；value 維持 `googleRoadmap` 以避免破壞既有狀態相容性。
- 使用者已確認 template 內有出現底圖選擇器。

### IFC Info Export

- 使用者希望能匯出 IFC info，提供 HTML / CSV 檢查細節。
- 新增 `ifc-info` CLI 子指令，可只讀 IFC 並輸出：
  - `ifc_info.html`
  - `ifc_info.json`
  - `ifc_products.csv`
  - `ifc_properties.csv`
  - `ifc_geometry_items.csv`
- 一般 IFC -> 3D Tiles 轉檔也會自動在 `out\<ifc-name>\` 產生同一組 IFC info，不需要額外再跑一次。
- report 目前涵蓋 product 清單、property set / single value、shape representation item、resolved geometry type、supported flag、converted flag、triangle count，以及 top entity type 統計。
