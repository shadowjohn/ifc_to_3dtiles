param(
  [string]$IndexPath = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\DJB-M-SU-_\index.html"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $IndexPath)) {
  throw "index.html 不存在：$IndexPath"
}

$html = Get-Content -LiteralPath $IndexPath -Raw
$checks = @(
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
  @{ Name = "Render scale 預設 0.5"; Pattern = 'value="0.5"' },
  @{ Name = "Render scale 滑桿"; Pattern = 'id="renderScaleSlider"' },
  @{ Name = "Render scale 自動"; Pattern = 'id="autoRenderScaleToggle"' },
  @{ Name = "Render scale 邏輯"; Pattern = 'applyRenderScale' },
  @{ Name = "移動中降畫質"; Pattern = 'configureAutoRenderScale' },
  @{ Name = "停止後回升畫質"; Pattern = 'restoreRenderScaleAfterMove' },
  @{ Name = "EMAP5 底圖"; Pattern = 'wmts.nlsc.gov.tw/wmts/EMAP5/default/GoogleMapsCompatible' },
  @{ Name = "EMAP5 初始化"; Pattern = 'setupEmap5Imagery' },
  @{ Name = "Google fallback"; Pattern = 'setupGoogleImagery' },
  @{ Name = "ArcGIS 操作手感"; Pattern = 'configureArcgisLikeNavigation' },
  @{ Name = "透地邏輯"; Pattern = 'setUndergroundEnabled' },
  @{ Name = "選取淡化"; Pattern = 'applySelectionStyle' },
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

$missing = New-Object System.Collections.Generic.List[string]
foreach ($check in $checks) {
  if ($html -notlike "*$($check.Pattern)*") {
    $missing.Add($check.Name)
  }
}

if ($missing.Count -gt 0) {
  Write-Error ("index.html 缺少功能入口：" + ($missing -join "、"))
  exit 1
}

Write-Host "index.html 功能入口檢查通過"
