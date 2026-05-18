# history.md

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
  - 圖台移動中暫時降到 0.5。
  - 停止後回到滑桿設定。

### Git 化

- 加入 `.gitignore`。
- 決定 source / tests / tools / docs / viewer shell 進 git。
- 忽略 IFC、`target/`、generated tiles、local Cesium package。
