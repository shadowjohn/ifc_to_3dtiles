param(
  [string]$Url = "http://127.0.0.1:8120/index.html?phase2a=1",
  [string]$PublishDir = "out\inspect_tamkang\publish"
)

$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent $PSScriptRoot
$Script = Join-Path $PSScriptRoot "phase2a_preview_screenshot.mjs"
$OutputDir = Join-Path $ProjectRoot (Join-Path $PublishDir "screenshots")
$ScreenshotPath = Join-Path $OutputDir "phase2a_preview.png"
$ReportPath = Join-Path $OutputDir "phase2a_visual_report.json"
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

Write-Host "[Phase 2A] url:        $Url"
Write-Host "[Phase 2A] output dir: $OutputDir"
Write-Host "[Phase 2A] screenshot: $ScreenshotPath"
Write-Host "[Phase 2A] report:     $ReportPath"
node $Script --url $Url --output-dir $OutputDir
if ($LASTEXITCODE -ne 0) {
  throw "phase2a screenshot capture failed with exit code $LASTEXITCODE"
}
