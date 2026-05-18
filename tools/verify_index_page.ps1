param(
  [string]$ViewerDir = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\DJB-M-SU-_",
  [string]$ProjectRoot = "C:\Users\stw_s\Desktop\ifc_to_3dtiles"
)

$ErrorActionPreference = "Stop"

function Assert-FileContains {
  param(
    [string]$Path,
    [array]$Checks
  )

  if (-not (Test-Path -LiteralPath $Path)) {
    throw "檔案不存在：$Path"
  }

  $content = Get-Content -LiteralPath $Path -Raw
  $missing = New-Object System.Collections.Generic.List[string]
  foreach ($check in $Checks) {
    if ($content -notlike "*$($check.Pattern)*") {
      $missing.Add($check.Name)
    }
  }

  if ($missing.Count -gt 0) {
    Write-Error ("$([System.IO.Path]::GetFileName($Path)) 缺少功能入口：" + ($missing -join "、"))
    exit 1
  }

  return $content
}

$indexPath = Join-Path $ViewerDir "index.html"
$threePath = Join-Path $ViewerDir "index_three.html"
$maplibrePath = Join-Path $ViewerDir "index_maplibre_three.html"
$commonPath = Join-Path $ViewerDir "three_viewer_common.js"
$vendorScriptPath = Join-Path $ProjectRoot "tools\prepare_three_vendor.ps1"

$indexChecks = @(
  @{ Name = "工具列"; Pattern = 'id="toolPanel"' },
  @{ Name = "工具列主操作列"; Pattern = 'primary-tools' },
  @{ Name = "工具列設定列"; Pattern = 'settings-tools' },
  @{ Name = "工具列分組"; Pattern = 'tool-group' },
  @{ Name = "爆炸滑桿"; Pattern = 'id="explodeSlider"' },
  @{ Name = "移出滑桿"; Pattern = 'id="moveOutSlider"' },
  @{ Name = "移出方向盤"; Pattern = 'id="movePad"' },
  @{ Name = "移出西南預設"; Pattern = 'data-move-direction="sw"' },
  @{ Name = "移出歸零"; Pattern = 'id="resetMoveOutButton"' },
  @{ Name = "焦點顯示開關"; Pattern = 'id="focusToggle"' },
  @{ Name = "透地開關"; Pattern = 'id="undergroundToggle"' },
  @{ Name = "底圖選擇"; Pattern = 'id="basemapSelect"' },
  @{ Name = "底圖切換邏輯"; Pattern = 'setBasemap' },
  @{ Name = "Google 電子地圖"; Pattern = 'googleRoadmap' },
  @{ Name = "OSM 底圖"; Pattern = 'tile.openstreetmap.org' },
  @{ Name = "法線平滑滑桿"; Pattern = 'id="normalModeSlider"' },
  @{ Name = "90 度平滑 tileset"; Pattern = 'tileset_smooth_90.json' },
  @{ Name = "平滑 tileset"; Pattern = 'tileset_smooth.json' },
  @{ Name = "平滑平面切換"; Pattern = 'setShadingMode' },
  @{ Name = "主 tileset lazy load"; Pattern = 'ensureMainTileset' },
  @{ Name = "量距按鈕"; Pattern = 'data-mode="distance"' },
  @{ Name = "量面按鈕"; Pattern = 'data-mode="area"' },
  @{ Name = "本機 Cesium"; Pattern = './Cesium-1.141/Build/Cesium/' },
  @{ Name = "Render scale 預設 1.0"; Pattern = 'value="1"' },
  @{ Name = "Render scale 滑桿"; Pattern = 'id="renderScaleSlider"' },
  @{ Name = "Render scale 自動"; Pattern = 'id="autoRenderScaleToggle"' },
  @{ Name = "Render scale 移動中 50"; Pattern = 'AUTO_RENDER_SCALE_MOVING' },
  @{ Name = "Render scale 停止後 100"; Pattern = 'AUTO_RENDER_SCALE_RESTORED' },
  @{ Name = "Render scale debug log gated"; Pattern = 'debugRenderScale' },
  @{ Name = "Render scale 邏輯"; Pattern = 'applyRenderScale' },
  @{ Name = "移動中降畫質"; Pattern = 'configureAutoRenderScale' },
  @{ Name = "輸入事件回復畫質"; Pattern = 'bindAutoRenderScaleInteractionFallback' },
  @{ Name = "停止後回升畫質"; Pattern = 'restoreRenderScaleAfterMove' },
  @{ Name = "EMAP5 底圖"; Pattern = 'wmts.nlsc.gov.tw/wmts/EMAP5/default/GoogleMapsCompatible' },
  @{ Name = "EMAP5 初始化"; Pattern = 'setupEmap5Imagery' },
  @{ Name = "Google fallback"; Pattern = 'setupGoogleImagery' },
  @{ Name = "ArcGIS 操作手感"; Pattern = 'configureArcgisLikeNavigation' },
  @{ Name = "透地邏輯"; Pattern = 'setUndergroundEnabled' },
  @{ Name = "選取淡化"; Pattern = 'applySelectionStyle' },
  @{ Name = "選取黃色高亮"; Pattern = 'buildSelectedOverlayStyle' },
  @{ Name = "焦點 OFF 選取仍可用"; Pattern = 'buildBaseSelectionStyle' },
  @{ Name = "焦點 OFF 爆炸仍可用"; Pattern = 'applySelectedExplosion(value)' },
  @{ Name = "焦點 OFF 移出仍可用"; Pattern = 'applySelectedMoveOut(value)' },
  @{ Name = "移出方向邏輯"; Pattern = 'setMoveDirection' },
  @{ Name = "移出 shader uniform"; Pattern = 'u_moveDistance' },
  @{ Name = "焦點顯示還原"; Pattern = 'clearFocusDisplay' },
  @{ Name = "選取實體 overlay"; Pattern = 'ensureSelectedTileset' },
  @{ Name = "選取物件爆炸"; Pattern = 'applySelectedExplosion' },
  @{ Name = "取消選取"; Pattern = 'clearFeatureSelection' },
  @{ Name = "爆炸邏輯"; Pattern = 'applyExplosion' },
  @{ Name = "量測結果"; Pattern = 'measureResult' }
)

$threeChecks = @(
  @{ Name = "本機 Three import map"; Pattern = './vendor/three/three.module.js' },
  @{ Name = "本機 OrbitControls prefix"; Pattern = 'three/addons/' },
  @{ Name = "本機 3d-tiles-renderer"; Pattern = './vendor/3d-tiles-renderer/build/index.three.js' },
  @{ Name = "焦點顯示開關"; Pattern = 'id="focusToggle"' },
  @{ Name = "爆炸滑桿"; Pattern = 'id="explodeSlider"' },
  @{ Name = "移出滑桿"; Pattern = 'id="moveOutSlider"' },
  @{ Name = "移出方向盤"; Pattern = 'id="movePad"' },
  @{ Name = "量距按鈕"; Pattern = 'data-measure-mode="distance"' },
  @{ Name = "量面按鈕"; Pattern = 'data-measure-mode="area"' },
  @{ Name = "量測結果"; Pattern = 'id="measureResult"' },
  @{ Name = "法線滑桿"; Pattern = 'id="normalModeSlider"' },
  @{ Name = "Render scale 滑桿"; Pattern = 'id="renderScaleSlider"' },
  @{ Name = "自動畫質"; Pattern = 'id="autoRenderScaleToggle"' },
  @{ Name = "效能面板"; Pattern = 'id="performancePanel"' },
  @{ Name = "共用 viewer module"; Pattern = './three_viewer_common.js' }
)

$maplibreChecks = @(
  @{ Name = "本機 MapLibre CSS"; Pattern = './vendor/maplibre/maplibre-gl.css' },
  @{ Name = "本機 MapLibre JS"; Pattern = './vendor/maplibre/maplibre-gl.js' },
  @{ Name = "本機 Three import map"; Pattern = './vendor/three/three.module.js' },
  @{ Name = "底圖選擇"; Pattern = 'id="basemapSelect"' },
  @{ Name = "EMAP5"; Pattern = 'value="emap5"' },
  @{ Name = "Google 航照"; Pattern = 'value="googleSatellite"' },
  @{ Name = "Google 電子地圖"; Pattern = 'value="googleRoadmap"' },
  @{ Name = "OSM"; Pattern = 'value="osm"' },
  @{ Name = "效能面板"; Pattern = 'id="performancePanel"' },
  @{ Name = "共用 viewer module"; Pattern = './three_viewer_common.js' }
)

$commonChecks = @(
  @{ Name = "TilesRenderer"; Pattern = 'TilesRenderer' },
  @{ Name = "OrbitControls"; Pattern = 'three/addons/controls/OrbitControls.js' },
  @{ Name = "Raycaster 選取"; Pattern = 'Raycaster' },
  @{ Name = "Batch Table 快取"; Pattern = 'batchTableCache' },
  @{ Name = "B3DM metadata parser"; Pattern = 'parseB3dmBatchTable' },
  @{ Name = "選取 overlay"; Pattern = 'buildSelectedOverlay' },
  @{ Name = "90 度平滑 tileset"; Pattern = 'tileset_smooth_90.json' },
  @{ Name = "平滑 tileset"; Pattern = 'tileset_smooth.json' },
  @{ Name = "焦點顯示邏輯"; Pattern = 'applyFocusDisplay' },
  @{ Name = "移動中 50%"; Pattern = 'AUTO_RENDER_SCALE_MOVING' },
  @{ Name = "停止後 100%"; Pattern = 'AUTO_RENDER_SCALE_RESTORED' },
  @{ Name = "純 Three 底圖替代層"; Pattern = 'createGroundLayer' },
  @{ Name = "MapLibre custom layer"; Pattern = 'type: "custom"' },
  @{ Name = "MapLibre camera sync"; Pattern = 'syncMaplibreCamera' },
  @{ Name = "Three custom render"; Pattern = 'renderingMode: "3d"' },
  @{ Name = "EMAP5"; Pattern = 'emap5' },
  @{ Name = "Google 航照"; Pattern = 'googleSatellite' },
  @{ Name = "Google 電子地圖"; Pattern = 'googleRoadmap' },
  @{ Name = "OSM"; Pattern = 'osm' }
)

$vendorScriptChecks = @(
  @{ Name = "Three 固定版本"; Pattern = '0.183.0' },
  @{ Name = "3d-tiles-renderer 固定版本"; Pattern = '0.4.21' },
  @{ Name = "MapLibre 固定版本"; Pattern = '5.24.0' },
  @{ Name = "vendor 目錄"; Pattern = 'vendor' },
  @{ Name = "retry"; Pattern = 'MaxAttempts' },
  @{ Name = "timeout"; Pattern = 'TimeoutSec' }
)

Assert-FileContains -Path $indexPath -Checks $indexChecks | Out-Null
$threeHtml = Assert-FileContains -Path $threePath -Checks $threeChecks
$maplibreHtml = Assert-FileContains -Path $maplibrePath -Checks $maplibreChecks
Assert-FileContains -Path $commonPath -Checks $commonChecks | Out-Null
Assert-FileContains -Path $vendorScriptPath -Checks $vendorScriptChecks | Out-Null

foreach ($page in @(@{ Name = "index_three.html"; Html = $threeHtml }, @{ Name = "index_maplibre_three.html"; Html = $maplibreHtml })) {
  if ($page.Html -match '(?i)(src|href)\s*=\s*["'']https?://') {
    Write-Error "$($page.Name) 不應直接載入 CDN runtime，請改走 ./vendor/ 相對路徑"
    exit 1
  }

  if ($page.Html -match '(?i)import\s+.*https?://') {
    Write-Error "$($page.Name) 不應從網路 import runtime，請改走 ./vendor/ 相對路徑"
    exit 1
  }
}

Write-Host "viewer 功能入口檢查通過"
