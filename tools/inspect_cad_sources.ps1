param(
  [Alias("Input")]
  [string]$InputPath = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\sample_files\淡江大橋移交模型",
  [string]$Output = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\cad_probe"
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
$tools = [ordered]@{
  ogrinfo = Get-ToolInfo @("ogrinfo", "ogrinfo.exe")
  ogr2ogr = Get-ToolInfo @("ogr2ogr", "ogr2ogr.exe")
  oda_file_converter = Get-ToolInfo @("ODAFileConverter", "ODAFileConverter.exe")
}
$tools.oda_file_converter["version_risk"] = Get-OdaVersionRisk $tools.oda_file_converter.version

$report = [ordered]@{
  input = $InputPath
  output = $Output
  generated_at = (Get-Date).ToString("o")
  tools = $tools
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
    Write-Host ("  {0}: FOUND - {1}" -f $toolName, $tool.source)
  } else {
    Write-Host ("  {0}: MISSING" -f $toolName)
  }
}

Write-Host ("Sample files: {0}, CAD files: {1}" -f $report.file_count, $report.cad_file_count)
Write-Host "Extension distribution:"
foreach ($item in $report.extension_distribution) {
  Write-Host ("  {0}: {1} files, {2} bytes" -f $item.extension, $item.count, $item.total_bytes)
}
