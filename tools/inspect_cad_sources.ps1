param(
  [Alias("Input")]
  [string]$InputPath = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\sample_files\淡江大橋移交模型",
  [string]$Output = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\cad_probe",
  [Alias("require-oda-major")]
  [int]$RequireOdaMajor = 0
)

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $Output | Out-Null

function Get-ToolInfo {
  param(
    [string[]]$Names
  )

  foreach ($name in $Names) {
    $cmd = Get-Command $name -ErrorAction SilentlyContinue
    if ($cmd) {
      $version = $null
      try {
        $version = (Get-Item -LiteralPath $cmd.Source).VersionInfo.FileVersion
      } catch {
        $version = $null
      }

      return [ordered]@{
        found = $true
        name = $cmd.Name
        source = $cmd.Source
        version = $version
      }
    }
  }

  return [ordered]@{
    found = $false
    name = $Names[0]
    source = $null
    version = $null
  }
}

function Get-OdaFileConverters {
  $candidatePaths = New-Object System.Collections.Generic.List[string]

  $override = $env:ODA_FILE_CONVERTER_EXE
  if ($override) {
    $candidatePaths.Add($override)
  }

  $cmd = Get-Command "ODAFileConverter.exe" -ErrorAction SilentlyContinue
  if ($cmd) {
    $candidatePaths.Add($cmd.Source)
  }

  $searchRoots = @(
    "C:\bin",
    "C:\Program Files\ODA"
  )

  foreach ($root in $searchRoots) {
    if (-not (Test-Path -LiteralPath $root)) {
      continue
    }

    Get-ChildItem -LiteralPath $root -Directory -Filter "ODAFileConverter*" -ErrorAction SilentlyContinue |
      ForEach-Object {
        $exe = Join-Path $_.FullName "ODAFileConverter.exe"
        if (Test-Path -LiteralPath $exe) {
          $candidatePaths.Add($exe)
        }
      }
  }

  $seen = @{}
  $converters = foreach ($path in $candidatePaths) {
    if (-not $path) {
      continue
    }

    try {
      $item = Get-Item -LiteralPath $path -ErrorAction Stop
    } catch {
      continue
    }

    $fullName = $item.FullName
    if ($seen.ContainsKey($fullName.ToLowerInvariant())) {
      continue
    }
    $seen[$fullName.ToLowerInvariant()] = $true

    $versionText = $item.VersionInfo.ProductVersion
    if (-not $versionText) {
      $versionText = $item.VersionInfo.FileVersion
    }

    $version = $null
    try {
      if ($versionText) {
        $version = [version]$versionText
      }
    } catch {
      $version = [version]"0.0.0.0"
    }

    [pscustomobject]@{
      found = $true
      name = $item.Name
      source = $fullName
      version = $versionText
      version_major = if ($version) { $version.Major } else { 0 }
      version_minor = if ($version) { $version.Minor } else { 0 }
      version_build = if ($version) { $version.Build } else { 0 }
      version_revision = if ($version) { $version.Revision } else { 0 }
      version_risk = Get-OdaVersionRisk $versionText
      last_write_time = $item.LastWriteTime.ToString("o")
    }
  }

  @($converters | Sort-Object `
    -Property @{ Expression = "version_major"; Descending = $true },
              @{ Expression = "version_minor"; Descending = $true },
              @{ Expression = "version_build"; Descending = $true },
              @{ Expression = "version_revision"; Descending = $true },
              @{ Expression = "source"; Descending = $false })
}

function Get-OdaVersionRisk {
  param([string]$Version)

  if (-not $Version) {
    return "unknown_version"
  }

  try {
    $parsed = [version]$Version
    if ($parsed.Major -lt 26) {
      return "too_old_for_2026_cad_delivery"
    }
    return "acceptable_baseline"
  } catch {
    return "unparseable_version"
  }
}

if (-not (Test-Path -LiteralPath $InputPath)) {
  throw "輸入目錄不存在：$InputPath"
}

$files = Get-ChildItem -LiteralPath $InputPath -Recurse -File |
  Sort-Object FullName |
  Select-Object FullName, Extension, Length

$extensionDistribution = $files |
  Group-Object { if ($_.Extension) { $_.Extension.ToLowerInvariant() } else { "[no_extension]" } } |
  Sort-Object -Property @{ Expression = "Count"; Descending = $true }, @{ Expression = "Name"; Ascending = $true } |
  ForEach-Object {
    [ordered]@{
      extension = $_.Name
      count = $_.Count
      total_bytes = ($_.Group | Measure-Object -Property Length -Sum).Sum
    }
  }

$cadFiles = $files | Where-Object { $_.Extension -match '^\.(dgn|dwg|dxf)$' }
$odaConverters = @(Get-OdaFileConverters)
$preferredOda = if ($odaConverters.Count -gt 0) { $odaConverters[0] } else {
  [pscustomobject]@{
    found = $false
    name = "ODAFileConverter.exe"
    source = $null
    version = $null
    version_major = 0
    version_minor = 0
    version_build = 0
    version_revision = 0
    version_risk = "missing"
    last_write_time = $null
  }
}
$tools = [ordered]@{
  ogrinfo = Get-ToolInfo @("ogrinfo", "ogrinfo.exe")
  ogr2ogr = Get-ToolInfo @("ogr2ogr", "ogr2ogr.exe")
  oda_file_converter = [ordered]@{
    found = [bool]$preferredOda.found
    name = $preferredOda.name
    source = $preferredOda.source
    version = $preferredOda.version
    version_risk = $preferredOda.version_risk
  }
}

$report = [ordered]@{
  input = $InputPath
  output = $Output
  generated_at = (Get-Date).ToString("o")
  required_oda_major = if ($RequireOdaMajor -gt 0) { $RequireOdaMajor } else { $null }
  tools = $tools
  oda_file_converters = @($odaConverters)
  preferred_oda_file_converter = $preferredOda
  file_count = $files.Count
  cad_file_count = $cadFiles.Count
  extension_distribution = @($extensionDistribution)
  cad_files = @($cadFiles)
  note = "這支 probe 只盤點本機工具與檔案分布，不執行轉檔，也不需要 Bentley 付費工具。"
}

$reportPath = Join-Path $Output "cad_probe_report.json"
$report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $reportPath -Encoding UTF8

Write-Host "CAD probe report: $reportPath"
Write-Host "Tools:"
foreach ($toolName in $report.tools.Keys) {
  $tool = $report.tools[$toolName]
  if ($tool.found) {
    if ($tool.version) {
      Write-Host ("  {0}: FOUND - {1} ({2}, {3})" -f $toolName, $tool.source, $tool.version, $tool.version_risk)
    } else {
      Write-Host ("  {0}: FOUND - {1}" -f $toolName, $tool.source)
    }
  } else {
    Write-Host ("  {0}: MISSING" -f $toolName)
  }
}

Write-Host "ODA File Converter candidates:"
foreach ($converter in $report.oda_file_converters) {
  Write-Host ("  {0} - {1} ({2})" -f $converter.version, $converter.source, $converter.version_risk)
}
Write-Host ("Preferred ODA File Converter: {0}" -f $report.preferred_oda_file_converter.source)

Write-Host ("Sample files: {0}, CAD files: {1}" -f $report.file_count, $report.cad_file_count)
Write-Host "Extension distribution:"
foreach ($item in $report.extension_distribution) {
  Write-Host ("  {0}: {1} files, {2} bytes" -f $item.extension, $item.count, $item.total_bytes)
}

if ($RequireOdaMajor -gt 0 -and (-not $preferredOda.found -or $preferredOda.version_major -lt $RequireOdaMajor)) {
  throw ("Preferred ODA File Converter is too old: required major {0}, detected {1} ({2})" -f `
    $RequireOdaMajor, $preferredOda.version, $preferredOda.source)
}
