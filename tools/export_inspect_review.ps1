param(
    [string]$InputDir = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\inspect_tamkang",
    [string]$OutputFile = "",
    [string]$CargoExe = ""
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($OutputFile)) {
    $OutputFile = Join-Path $InputDir "review_report.html"
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

Write-Host "[Phase 1D] Export inspect review"
Write-Host "  input   : $InputDir"
Write-Host "  output  : $OutputFile"
Write-Host "  cargo   : $CargoExe"

Push-Location $RepoRoot
try {
    & $CargoExe run -- inspect-review --input $InputDir --output $OutputFile
    if ($LASTEXITCODE -ne 0) {
        throw "inspect-review failed with exit code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}

Write-Host "[Phase 1D] Review report ready: $OutputFile"
