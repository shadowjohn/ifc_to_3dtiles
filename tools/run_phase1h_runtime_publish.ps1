param(
    [string]$ProjectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path,
    [string]$InputDir = "",
    [string]$OutputDir = "",
    [string]$CargoExe = ""
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($InputDir)) {
    $InputDir = Join-Path $ProjectRoot "out\inspect_tamkang"
}
if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $OutputDir = Join-Path $InputDir "publish"
}
if ([string]::IsNullOrWhiteSpace($CargoExe)) {
    $CargoExe = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
}

if (-not (Test-Path -LiteralPath $CargoExe)) {
    throw "找不到 cargo：$CargoExe"
}
if (-not (Test-Path -LiteralPath (Join-Path $InputDir "qa\approved_sources.json"))) {
    throw "找不到 approved_sources.json，請先跑 Phase 1E：$InputDir\qa\approved_sources.json"
}
if (-not (Test-Path -LiteralPath (Join-Path $InputDir "project_inspect.db"))) {
    throw "找不到 project_inspect.db，請先完成 Phase 1C entity inspect：$InputDir\project_inspect.db"
}

Write-Host "[Phase 1H] project root: $ProjectRoot"
Write-Host "[Phase 1H] input:        $InputDir"
Write-Host "[Phase 1H] output:       $OutputDir"
Write-Host "[Phase 1H] regenerate Phase 1F/1G publish viewer"
Push-Location $ProjectRoot
try {
    & $CargoExe run -- publish-approved --input $InputDir --output $OutputDir
    if ($LASTEXITCODE -ne 0) {
        throw "publish-approved failed with exit code $LASTEXITCODE"
    }

    Write-Host "[Phase 1H] build approved-only runtime geometry"
    & $CargoExe run -- runtime-publish --input $InputDir --output $OutputDir
    if ($LASTEXITCODE -ne 0) {
        throw "runtime-publish failed with exit code $LASTEXITCODE"
    }
}
finally {
    Pop-Location
}

$expected = @(
    "runtime_manifest.json",
    "runtime_budget_report.json",
    "spatial_pick_index.json",
    "runtime_metadata\dwg-12d5f1b6.json",
    "runtime\dwg-12d5f1b6\runtime.glb",
    "runtime\dwg-12d5f1b6\runtime_metadata.json",
    "runtime\dwg-12d5f1b6\runtime_pick.json"
)

foreach ($relative in $expected) {
    $path = Join-Path $OutputDir $relative
    if (-not (Test-Path -LiteralPath $path)) {
        throw "Phase 1H expected output missing: $path"
    }
}

Write-Host "[Phase 1H] outputs:"
foreach ($relative in $expected) {
    Write-Host "  $(Join-Path $OutputDir $relative)"
}
Write-Host "[Phase 1H] viewer:"
Write-Host "  $(Join-Path $OutputDir "index.html")"
