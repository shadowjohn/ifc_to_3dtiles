param(
  [string]$ProjectRoot = "C:\Users\stw_s\Desktop\ifc_to_3dtiles",
  [string]$InputDir = "",
  [string]$OutputDir = ""
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($InputDir)) {
  $InputDir = Join-Path $ProjectRoot "out\inspect_tamkang"
}
if ([string]::IsNullOrWhiteSpace($OutputDir)) {
  $OutputDir = Join-Path $InputDir "publish"
}

$CargoExe = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
if (-not (Test-Path -LiteralPath $CargoExe)) {
  $CargoExe = "cargo"
}

Write-Host "[Phase 1K] project root: $ProjectRoot"
Write-Host "[Phase 1K] input:        $InputDir"
Write-Host "[Phase 1K] output:       $OutputDir"

Push-Location $ProjectRoot
try {
  Write-Host "[Phase 1K] regenerate publish viewer"
  & $CargoExe run -- publish-approved --input $InputDir --output $OutputDir
  if ($LASTEXITCODE -ne 0) {
    throw "publish-approved failed with exit code $LASTEXITCODE"
  }

  Write-Host "[Phase 1K] keep runtime pick index current"
  & $CargoExe run -- runtime-publish --input $InputDir --output $OutputDir
  if ($LASTEXITCODE -ne 0) {
    throw "runtime-publish failed with exit code $LASTEXITCODE"
  }

  Write-Host "[Phase 1K] build minimal geometry preview"
  & $CargoExe run -- geometry-preview --input $InputDir --output $OutputDir
  if ($LASTEXITCODE -ne 0) {
    throw "geometry-preview failed with exit code $LASTEXITCODE"
  }
} finally {
  Pop-Location
}

$PreviewDir = Join-Path $OutputDir "geometry_preview"
Write-Host "[Phase 1K] outputs:"
Write-Host "  $(Join-Path $PreviewDir 'raw.glb')"
Write-Host "  $(Join-Path $PreviewDir 'tile.glb')"
Write-Host "  $(Join-Path $PreviewDir 'tileset.json')"
Write-Host "  $(Join-Path $PreviewDir 'geometry_publish_report.json')"
