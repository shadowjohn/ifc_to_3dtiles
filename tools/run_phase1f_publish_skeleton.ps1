param(
    [string]$InputDir = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\inspect_tamkang",
    [string]$OutputDir = "",
    [string]$CargoExe = ""
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $OutputDir = Join-Path $InputDir "publish"
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

$qaDir = Join-Path $InputDir "qa"
foreach ($name in @("approved_sources.json", "rejected_sources.json", "needs_review_sources.json")) {
    $path = Join-Path $qaDir $name
    if (-not (Test-Path -LiteralPath $path)) {
        throw "缺少 Phase 1E approval manifest：$path"
    }
}

Write-Host "[Phase 1F] Approved source publish skeleton"
Write-Host "  input   : $InputDir"
Write-Host "  output  : $OutputDir"
Write-Host "  cargo   : $CargoExe"

Push-Location $RepoRoot
try {
    & $CargoExe run -- publish-approved --input $InputDir --output $OutputDir
    if ($LASTEXITCODE -ne 0) {
        throw "publish-approved failed with exit code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}

$expectedFiles = @(
    "sources_manifest.json",
    "debug_overlays.json",
    "index.html"
)
foreach ($name in $expectedFiles) {
    $path = Join-Path $OutputDir $name
    if (-not (Test-Path -LiteralPath $path)) {
        throw "Phase 1F 缺少輸出檔：$path"
    }
}

Write-Host "[Phase 1F] Publish skeleton ready: $OutputDir"
