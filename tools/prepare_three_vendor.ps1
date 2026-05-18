param(
  [string]$ViewerDir = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\DJB-M-SU-_",
  [int]$MaxAttempts = 3,
  [int]$TimeoutSec = 120
)

$ErrorActionPreference = "Stop"

$threeVersion = "0.183.0"
$tilesVersion = "0.4.21"
$maplibreVersion = "5.24.0"

$vendorDir = Join-Path $ViewerDir "vendor"
$tempRoot = Join-Path $env:TEMP ("ifc_three_vendor_" + [guid]::NewGuid().ToString("N"))

function Invoke-WithRetry {
  param(
    [scriptblock]$ScriptBlock,
    [string]$Label
  )

  for ($attempt = 1; $attempt -le $MaxAttempts; $attempt++) {
    try {
      Write-Host "[$attempt/$MaxAttempts] $Label"
      return & $ScriptBlock
    } catch {
      if ($attempt -eq $MaxAttempts) {
        throw
      }
      Start-Sleep -Seconds ([Math]::Min(10, 2 * $attempt))
    }
  }
}

function Expand-NpmPackage {
  param(
    [string]$PackageName,
    [string]$Version
  )

  $packageWorkDir = Join-Path $tempRoot ($PackageName -replace '[\\/@]', '_')
  New-Item -ItemType Directory -Force -Path $packageWorkDir | Out-Null

  Invoke-WithRetry -Label "npm pack $PackageName@$Version" -ScriptBlock {
    $env:npm_config_fetch_timeout = ($TimeoutSec * 1000).ToString()
    npm pack "$PackageName@$Version" --pack-destination $packageWorkDir --silent | Out-Null
    if ($LASTEXITCODE -ne 0) {
      throw "npm pack $PackageName@$Version 失敗"
    }
  } | Out-Null

  $tgz = Get-ChildItem -LiteralPath $packageWorkDir -Filter "*.tgz" | Select-Object -First 1
  if (-not $tgz) {
    throw "找不到 npm tarball：$PackageName@$Version"
  }

  tar -xzf $tgz.FullName -C $packageWorkDir
  if ($LASTEXITCODE -ne 0) {
    throw "解壓縮失敗：$($tgz.FullName)"
  }

  return (Join-Path $packageWorkDir "package")
}

function Copy-CleanDirectory {
  param(
    [string]$Source,
    [string]$Destination
  )

  if (Test-Path -LiteralPath $Destination) {
    Remove-Item -LiteralPath $Destination -Recurse -Force
  }
  New-Item -ItemType Directory -Force -Path $Destination | Out-Null
  Copy-Item -Path (Join-Path $Source "*") -Destination $Destination -Recurse -Force
}

try {
  New-Item -ItemType Directory -Force -Path $vendorDir | Out-Null
  New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null

  $threePackage = Expand-NpmPackage -PackageName "three" -Version $threeVersion
  $tilesPackage = Expand-NpmPackage -PackageName "3d-tiles-renderer" -Version $tilesVersion
  $maplibrePackage = Expand-NpmPackage -PackageName "maplibre-gl" -Version $maplibreVersion

  $threeVendor = Join-Path $vendorDir "three"
  if (Test-Path -LiteralPath $threeVendor) {
    Remove-Item -LiteralPath $threeVendor -Recurse -Force
  }
  New-Item -ItemType Directory -Force -Path (Join-Path $threeVendor "examples") | Out-Null
  New-Item -ItemType Directory -Force -Path $threeVendor | Out-Null
  # three.module.js 在 r183 會相對載入 three.core.js，build 目錄一併同步比較穩。
  Copy-Item -Path (Join-Path $threePackage "build\*.js") -Destination $threeVendor -Force
  Copy-Item -LiteralPath (Join-Path $threePackage "examples\jsm") -Destination (Join-Path $threeVendor "examples\jsm") -Recurse -Force

  # 3d-tiles-renderer 的 build 會引用同包 chunks；保留整包可避免後續版本路徑漏拷。
  Copy-CleanDirectory -Source $tilesPackage -Destination (Join-Path $vendorDir "3d-tiles-renderer")

  $maplibreVendor = Join-Path $vendorDir "maplibre"
  if (Test-Path -LiteralPath $maplibreVendor) {
    Remove-Item -LiteralPath $maplibreVendor -Recurse -Force
  }
  New-Item -ItemType Directory -Force -Path $maplibreVendor | Out-Null
  Copy-Item -LiteralPath (Join-Path $maplibrePackage "dist\maplibre-gl.js") -Destination (Join-Path $maplibreVendor "maplibre-gl.js") -Force
  Copy-Item -LiteralPath (Join-Path $maplibrePackage "dist\maplibre-gl.css") -Destination (Join-Path $maplibreVendor "maplibre-gl.css") -Force

  $manifest = [ordered]@{
    three = $threeVersion
    three_files = @("three.module.js", "examples/jsm")
    "3d-tiles-renderer" = $tilesVersion
    "maplibre-gl" = $maplibreVersion
    prepared_at = (Get-Date).ToString("s")
  }
  $manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $vendorDir "manifest.json") -Encoding UTF8

  Write-Host "vendor 已準備完成：$vendorDir"
} finally {
  if (Test-Path -LiteralPath $tempRoot) {
    Remove-Item -LiteralPath $tempRoot -Recurse -Force
  }
}
