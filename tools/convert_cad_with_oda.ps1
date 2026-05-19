param(
  [string]$Manifest = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\inspect_tamkang\source_manifest.json",
  [string]$ProbeReport = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\cad_probe\cad_probe_report.json",
  [string]$Output = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\oda_normalized",
  [string]$TargetVersion = "ACAD2018",
  [ValidateSet("DWG", "DXF")]
  [string]$TargetFormat = "DWG",
  [string]$SourceId = "",
  [ValidateSet("", "dwg", "dgn")]
  [string]$InputFormatFilter = "",
  [string]$ReportName = "oda_conversion_report.json",
  [int]$Limit = 0,
  [int]$TimeoutMinutes = 30
)

$ErrorActionPreference = "Stop"

function Get-RequiredProperty {
  param(
    [object]$Object,
    [string]$Name
  )

  $property = $Object.PSObject.Properties[$Name]
  if ($null -eq $property -or $null -eq $property.Value) {
    throw "缺少必要欄位：$Name"
  }
  return $property.Value
}

function Get-OptionalText {
  param(
    [object]$Object,
    [string]$Name,
    [string]$Fallback
  )

  $property = $Object.PSObject.Properties[$Name]
  if ($null -ne $property -and $null -ne $property.Value -and "$($property.Value)" -ne "") {
    return [string]$property.Value
  }
  return $Fallback
}

function Get-FileSha256 {
  param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) {
    return $null
  }
  return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Invoke-ProcessCapture {
  param(
    [string]$FileName,
    [string[]]$Arguments,
    [int]$TimeoutMinutes
  )

  $psi = [System.Diagnostics.ProcessStartInfo]::new()
  $psi.FileName = $FileName
  $psi.UseShellExecute = $false
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $psi.CreateNoWindow = $true
  foreach ($argument in $Arguments) {
    [void]$psi.ArgumentList.Add($argument)
  }

  $process = [System.Diagnostics.Process]::new()
  $process.StartInfo = $psi
  [void]$process.Start()
  $stdoutTask = $process.StandardOutput.ReadToEndAsync()
  $stderrTask = $process.StandardError.ReadToEndAsync()
  $timeoutMs = [Math]::Max(1, $TimeoutMinutes) * 60 * 1000
  $timedOut = -not $process.WaitForExit($timeoutMs)
  if ($timedOut) {
    try {
      $process.Kill($true)
    } catch {
      $process.Kill()
    }
    $process.WaitForExit()
  }

  return [pscustomobject]@{
    exit_code = if ($timedOut) { -1 } else { $process.ExitCode }
    timed_out = $timedOut
    stdout = $stdoutTask.GetAwaiter().GetResult()
    stderr = $stderrTask.GetAwaiter().GetResult()
  }
}

function Find-ConvertedCadFile {
  param(
    [string]$Directory,
    [string]$InputPath,
    [string]$TargetFormat
  )

  if (-not (Test-Path -LiteralPath $Directory)) {
    return $null
  }

  $targetExtension = "." + $TargetFormat.ToLowerInvariant()
  $inputStem = [System.IO.Path]::GetFileNameWithoutExtension($InputPath)
  $candidates = @(Get-ChildItem -LiteralPath $Directory -Recurse -File -ErrorAction SilentlyContinue |
    Where-Object { $_.Extension.Equals($targetExtension, [System.StringComparison]::OrdinalIgnoreCase) } |
    Sort-Object LastWriteTimeUtc -Descending)

  $exact = $candidates | Where-Object {
    [System.IO.Path]::GetFileNameWithoutExtension($_.Name) -eq $inputStem
  } | Select-Object -First 1
  if ($exact) {
    return $exact.FullName
  }
  if ($candidates.Count -gt 0) {
    return $candidates[0].FullName
  }
  return $null
}

function Get-OdaErrorLogWarnings {
  param([string]$Directory)

  if (-not (Test-Path -LiteralPath $Directory)) {
    return @()
  }

  $logs = @(Get-ChildItem -LiteralPath $Directory -Recurse -File -Filter "*.err" -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTimeUtc -Descending)
  $warnings = @()
  foreach ($log in $logs) {
    $text = (Get-Content -LiteralPath $log.FullName -Raw -ErrorAction SilentlyContinue)
    if ($text) {
      $compact = ($text.Trim() -replace "\s+", " ")
      $warnings += ("ODA error log {0}: {1}" -f $log.Name, $compact)
    } else {
      $warnings += ("ODA error log exists: {0}" -f $log.Name)
    }
  }
  return $warnings
}

if (-not (Test-Path -LiteralPath $Manifest)) {
  throw "manifest 不存在：$Manifest"
}
if (-not (Test-Path -LiteralPath $ProbeReport)) {
  throw "CAD probe report 不存在：$ProbeReport"
}

$manifestObject = Get-Content -LiteralPath $Manifest -Raw | ConvertFrom-Json
$probeObject = Get-Content -LiteralPath $ProbeReport -Raw | ConvertFrom-Json
$preferredOda = Get-RequiredProperty $probeObject "preferred_oda_file_converter"
$odaExe = Get-RequiredProperty $preferredOda "source"
$odaVersion = Get-OptionalText $preferredOda "version" $null
if (-not (Test-Path -LiteralPath $odaExe)) {
  throw "ODA File Converter 不存在：$odaExe"
}

New-Item -ItemType Directory -Force -Path $Output | Out-Null
$targetFormatUpper = $TargetFormat.ToUpperInvariant()
$targetFormatLower = $TargetFormat.ToLowerInvariant()
$cadSources = @($manifestObject.sources | Where-Object {
  $_.format -eq "dgn" -or $_.format -eq "dwg"
})
if ($InputFormatFilter) {
  $cadSources = @($cadSources | Where-Object { $_.format -eq $InputFormatFilter })
}
if ($SourceId) {
  $cadSources = @($cadSources | Where-Object { $_.id -eq $SourceId })
  if ($cadSources.Count -eq 0) {
    throw "找不到指定 source_id：$SourceId"
  }
}
if ($Limit -gt 0) {
  $cadSources = @($cadSources | Select-Object -First $Limit)
}

$entries = @()
$index = 0
foreach ($source in $cadSources) {
  $index++
  $sourceId = Get-RequiredProperty $source "id"
  $inputPath = Get-RequiredProperty $source "path"
  $sourceOutput = Join-Path $Output $sourceId
  New-Item -ItemType Directory -Force -Path $sourceOutput | Out-Null

  $displayName = Get-OptionalText $source "display_name" ([System.IO.Path]::GetFileNameWithoutExtension($inputPath))
  $originalFileName = Get-OptionalText $source "original_file_name" ([System.IO.Path]::GetFileName($inputPath))
  $relativePath = Get-OptionalText $source "relative_path" $originalFileName
  $inputFormat = Get-OptionalText $source "format" ([System.IO.Path]::GetExtension($inputPath).TrimStart(".").ToLowerInvariant())
  $sourceWarnings = @()
  if ($source.PSObject.Properties["warnings"] -and $source.warnings) {
    $sourceWarnings = @($source.warnings | ForEach-Object { [string]$_ })
  }

  $inputDir = Split-Path -Parent $inputPath
  $filter = [System.IO.Path]::GetFileName($inputPath)
  $arguments = @(
    $inputDir,
    $sourceOutput,
    $TargetVersion,
    $targetFormatUpper,
    "0",
    "1",
    $filter
  )
  $command = @($odaExe) + $arguments

  Write-Host ("[{0}/{1}] ODA convert {2}" -f $index, $cadSources.Count, $originalFileName)
  $result = Invoke-ProcessCapture -FileName $odaExe -Arguments $arguments -TimeoutMinutes $TimeoutMinutes
  $convertedPath = Find-ConvertedCadFile -Directory $sourceOutput -InputPath $inputPath -TargetFormat $targetFormatUpper

  $warnings = @()
  foreach ($warning in $sourceWarnings) {
    $warnings += $warning
  }
  if ($result.timed_out) {
    $warnings += "ODA conversion timed out"
  }
  if ($result.exit_code -ne 0) {
    $warnings += ("ODA conversion exit code: {0}" -f $result.exit_code)
  }
  if ($result.stderr) {
    $warnings += ("ODA stderr: {0}" -f ($result.stderr.Trim() -replace "\s+", " "))
  }
  foreach ($warning in (Get-OdaErrorLogWarnings -Directory $sourceOutput)) {
    $warnings += $warning
  }
  if (-not $convertedPath) {
    $warnings += "ODA conversion did not produce a converted CAD file"
  }

  $success = ($result.exit_code -eq 0 -and $null -ne $convertedPath)
  $entry = [ordered]@{
    source_id = $sourceId
    source_display_name = $displayName
    source_original_file_name = $originalFileName
    source_relative_path = $relativePath
    input_path = $inputPath
    input_format = $inputFormat
    converted_path = $convertedPath
    converted_format = if ($convertedPath) { $targetFormatLower } else { $null }
    oda_version = $odaVersion
    target_version = $TargetVersion
    target_format = $targetFormatUpper
    success = $success
    status = if ($success) { "success" } else { "failed" }
    input_sha256 = Get-FileSha256 $inputPath
    converted_sha256 = if ($convertedPath) { Get-FileSha256 $convertedPath } else { $null }
    bbox_before = $source.raw_bbox
    bbox_after = [ordered]@{
      raw = $null
      percentile = $null
    }
    level_count_after = $null
    material_count_after = $null
    fingerprint_after = $null
    warnings = @($warnings)
    command = $command
    exit_code = $result.exit_code
  }
  $entries += [pscustomobject]$entry
}

$successCount = 0
$failedCount = 0
foreach ($entry in $entries) {
  if ($entry.success) {
    $successCount++
  } else {
    $failedCount++
  }
}

$report = [ordered]@{
  generated_at = (Get-Date).ToString("o")
  manifest_path = $Manifest
  probe_report_path = $ProbeReport
  output_path = $Output
  oda_exe = $odaExe
  oda_version = $odaVersion
  target_version = $TargetVersion
  target_format = $targetFormatUpper
  attempted_count = @($entries).Count
  success_count = $successCount
  failed_count = $failedCount
  entries = @($entries)
}

$reportPath = Join-Path $Output $ReportName
$report | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $reportPath -Encoding UTF8
Write-Host "ODA conversion report: $reportPath"
Write-Host ("Attempted: {0}, success: {1}, failed: {2}" -f $report.attempted_count, $report.success_count, $report.failed_count)
