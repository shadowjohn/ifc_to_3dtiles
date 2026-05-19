param(
    [string]$InputDir = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\inspect_tamkang",
    [string]$OutputDir = "",
    [string]$CargoExe = ""
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $OutputDir = Join-Path $InputDir "qa"
}
if ([string]::IsNullOrWhiteSpace($CargoExe)) {
    $cargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
    if ($cargoCommand) {
        $CargoExe = $cargoCommand.Source
    } else {
        $CargoExe = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    }
}
if (-not (Test-Path -LiteralPath $CargoExe)) {
    throw "找不到 cargo：$CargoExe"
}
if (-not (Test-Path -LiteralPath $InputDir)) {
    throw "找不到 inspect 輸入目錄：$InputDir"
}

$dbPath = Join-Path $InputDir "project_inspect.db"
$manifestPath = Join-Path $InputDir "source_manifest.json"
if (-not (Test-Path -LiteralPath $dbPath)) {
    throw "找不到 SQLite inspect DB：$dbPath"
}
if (-not (Test-Path -LiteralPath $manifestPath)) {
    throw "找不到 source manifest：$manifestPath"
}

Write-Host "[Phase 1E] Quarantine drilldown + approval workflow"
Write-Host "  input   : $InputDir"
Write-Host "  output  : $OutputDir"
Write-Host "  cargo   : $CargoExe"

Push-Location $RepoRoot
try {
    & $CargoExe run -- inspect-drilldown --input $InputDir --output $OutputDir
    if ($LASTEXITCODE -ne 0) {
        throw "inspect-drilldown failed with exit code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}

$expectedFiles = @(
    "duplicate_pairs.json",
    "entity_outliers.json",
    "approved_sources.json",
    "rejected_sources.json",
    "needs_review_sources.json"
)
foreach ($name in $expectedFiles) {
    $path = Join-Path $OutputDir $name
    if (-not (Test-Path -LiteralPath $path)) {
        throw "Phase 1E 缺少輸出檔：$path"
    }
}

Write-Host "[Phase 1E] QA outputs ready: $OutputDir"
