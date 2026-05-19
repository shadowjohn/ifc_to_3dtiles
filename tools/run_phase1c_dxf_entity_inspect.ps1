param(
  [string]$ProjectRoot = "C:\Users\stw_s\Desktop\ifc_to_3dtiles",
  [string]$InputPath = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\sample_files\淡江大橋移交模型",
  [string]$Output = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\inspect_tamkang",
  [int]$SourceEpsg = 3826,
  [string]$TargetVersion = "ACAD2000",
  [ValidateSet("DXF")]
  [string]$TargetFormat = "DXF",
  [int]$BatchSize = 0,
  [int]$TimeoutMinutes = 30
)

$ErrorActionPreference = "Stop"

$cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
if (-not (Test-Path -LiteralPath $cargo)) {
  throw "找不到 cargo：$cargo"
}

$manifest = Join-Path $Output "source_manifest.json"
$probeOutput = Join-Path $Output "cad_probe"
$probeReport = Join-Path $probeOutput "cad_probe_report.json"
$conversionReport = Join-Path $Output "cad_conversion_report.json"
$normalizedReport = Join-Path $Output "normalized_cad_inspect_report.json"
$entityReport = Join-Path $Output "entity_inspect_report.json"

Push-Location $ProjectRoot
try {
  Write-Host "Phase 1C step 1/5: source inspect"
  & $cargo run -- inspect --input $InputPath --output $Output --source-epsg $SourceEpsg
  if ($LASTEXITCODE -ne 0) {
    throw "source inspect failed"
  }

  Write-Host "Phase 1C step 2/5: CAD probe"
  pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\inspect_cad_sources.ps1 `
    --input $InputPath `
    -Output $probeOutput `
    --require-oda-major 27
  if ($LASTEXITCODE -ne 0) {
    throw "CAD probe failed"
  }

  Write-Host "Phase 1C step 3/5: DWG -> DXF via ODA"
  pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\convert_cad_with_oda.ps1 `
    -Manifest $manifest `
    -ProbeReport $probeReport `
    -Output $Output `
    -TargetVersion $TargetVersion `
    -TargetFormat $TargetFormat `
    -InputFormatFilter dwg `
    -ReportName "cad_conversion_report.json" `
    -TimeoutMinutes $TimeoutMinutes
  if ($LASTEXITCODE -ne 0) {
    throw "ODA DXF conversion failed"
  }

  Write-Host "Phase 1C step 4/5: DXF layer-level evidence report"
  pwsh -NoProfile -ExecutionPolicy Bypass -File .\tools\inspect_normalized_cad.ps1 `
    -ConversionReport $conversionReport `
    -Output $Output `
    -TimeoutSeconds ([Math]::Max(60, $TimeoutMinutes * 60))
  if ($LASTEXITCODE -ne 0) {
    throw "normalized CAD inspect failed"
  }

  Write-Host "Phase 1C step 5/5: DXF entity-level inspect"
  & $cargo run -- entity-inspect-dxf `
    --conversion-report $conversionReport `
    --manifest $manifest `
    --output $Output `
    --batch-size $BatchSize
  if ($LASTEXITCODE -ne 0) {
    throw "entity-level inspect failed"
  }

  Write-Host "Phase 1C completed:"
  Write-Host "  manifest: $manifest"
  Write-Host "  conversion: $conversionReport"
  Write-Host "  normalized evidence: $normalizedReport"
  Write-Host "  entity report: $entityReport"
  Write-Host ("  sqlite: {0}" -f (Join-Path $Output "project_inspect.db"))
} finally {
  Pop-Location
}
