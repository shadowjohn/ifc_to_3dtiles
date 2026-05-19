param(
  [string]$ConversionReport = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\oda_normalized\oda_conversion_report.json",
  [string]$Output = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\oda_normalized",
  [double[]]$AllowedScales = @(1000.0, 1.0, 0.1, 0.01, 0.001),
  [int]$TimeoutSeconds = 120
)

$ErrorActionPreference = "Stop"

function Invoke-ProcessCapture {
  param(
    [string]$FileName,
    [string[]]$Arguments,
    [int]$TimeoutSeconds
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
  $timedOut = -not $process.WaitForExit([Math]::Max(1, $TimeoutSeconds) * 1000)
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

function Get-LayerNames {
  param([string]$Text)

  $names = @()
  foreach ($line in ($Text -split "`r?`n")) {
    if ($line -match '^\s*\d+:\s+(.+?)(?:\s+\(|$)') {
      $names += $matches[1].Trim()
    }
  }
  return $names
}

function Get-Extent2D {
  param([string]$Text)

  foreach ($line in ($Text -split "`r?`n")) {
    if ($line -match 'Extent:\s*\(([-+0-9.eE]+),\s*([-+0-9.eE]+)\)\s*-\s*\(([-+0-9.eE]+),\s*([-+0-9.eE]+)\)') {
      return [double[]]@(
        [double]$matches[1],
        [double]$matches[2],
        [double]$matches[3],
        [double]$matches[4]
      )
    }
  }
  return $null
}

function Merge-Extent2D {
  param(
    [object]$Current,
    [object]$Next
  )

  if ($null -eq $Next) {
    return $Current
  }
  if ($null -eq $Current) {
    return $Next
  }

  return [double[]]@(
    [Math]::Min([double]$Current[0], [double]$Next[0]),
    [Math]::Min([double]$Current[1], [double]$Next[1]),
    [Math]::Max([double]$Current[2], [double]$Next[2]),
    [Math]::Max([double]$Current[3], [double]$Next[3])
  )
}

function Get-ScaleCandidates {
  param(
    [object]$Extent,
    [double[]]$Scales
  )

  if ($null -eq $Extent) {
    return @()
  }

  $centerX = ([double]$Extent[0] + [double]$Extent[2]) / 2.0
  $centerY = ([double]$Extent[1] + [double]$Extent[3]) / 2.0
  $candidates = @()
  foreach ($scale in $Scales) {
    $x = $centerX * $scale
    $y = $centerY * $scale
    if ($x -ge 120000.0 -and $x -le 360000.0 -and $y -ge 2400000.0 -and $y -le 2800000.0) {
      $candidates += $scale
    }
  }
  return $candidates
}

if (-not (Test-Path -LiteralPath $ConversionReport)) {
  throw "ODA conversion report 不存在：$ConversionReport"
}

$ogrinfo = Get-Command "ogrinfo.exe" -ErrorAction SilentlyContinue
if (-not $ogrinfo) {
  $ogrinfo = Get-Command "ogrinfo" -ErrorAction SilentlyContinue
}
$ogrinfoPath = if ($ogrinfo) { $ogrinfo.Source } else { $null }

$conversion = Get-Content -LiteralPath $ConversionReport -Raw | ConvertFrom-Json
$entries = @()
foreach ($entry in $conversion.entries) {
  $warnings = @()
  $convertedPath = [string]$entry.converted_path
  $command = $null
  $exitCode = $null
  $inspectSuccess = $false
  $bboxAfter = $null
  $scaleCandidates = @()
  $layerCount = $null

  if (-not $entry.success -or -not $convertedPath) {
    $warnings += "conversion failed or converted_path is missing"
  } elseif (-not $ogrinfoPath) {
    $warnings += "ogrinfo is missing"
  } elseif (-not (Test-Path -LiteralPath $convertedPath)) {
    $warnings += "converted CAD file does not exist"
  } else {
    $command = @($ogrinfoPath, "-so", $convertedPath)
    Write-Host ("OGR inspect {0}" -f $entry.source_original_file_name)
    $summary = Invoke-ProcessCapture -FileName $ogrinfoPath -Arguments @("-so", $convertedPath) -TimeoutSeconds $TimeoutSeconds
    $exitCode = $summary.exit_code
    if ($summary.timed_out) {
      $warnings += "ogrinfo timed out"
    }
    if ($summary.exit_code -ne 0) {
      $warnings += ("ogrinfo exit code: {0}" -f $summary.exit_code)
    }
    if ($summary.stderr) {
      $warnings += ("ogrinfo stderr: {0}" -f ($summary.stderr.Trim() -replace "\s+", " "))
    }

    $layers = @(Get-LayerNames -Text $summary.stdout)
    $layerCount = $layers.Count
    $mergedExtent = $null
    foreach ($layer in $layers) {
      $layerResult = Invoke-ProcessCapture -FileName $ogrinfoPath -Arguments @("-so", $convertedPath, $layer) -TimeoutSeconds $TimeoutSeconds
      if ($layerResult.exit_code -eq 0) {
        $mergedExtent = Merge-Extent2D -Current $mergedExtent -Next (Get-Extent2D -Text $layerResult.stdout)
      }
    }

    if ($mergedExtent) {
      $scaleCandidates = @(Get-ScaleCandidates -Extent $mergedExtent -Scales $AllowedScales)
      $bboxAfter = [ordered]@{
        raw = $null
        raw_2d = @($mergedExtent)
        percentile = $null
      }
      $inspectSuccess = $true
    } else {
      $warnings += "ogrinfo did not expose a usable layer extent"
    }
  }

  $entries += [pscustomobject][ordered]@{
    source_id = $entry.source_id
    source_original_file_name = $entry.source_original_file_name
    converted_path = $convertedPath
    inspect_success = $inspectSuccess
    ogrinfo_path = $ogrinfoPath
    exit_code = $exitCode
    bbox_before = $entry.bbox_before
    bbox_after = $bboxAfter
    scale_candidates_after = @($scaleCandidates)
    level_count_after = $layerCount
    material_count_after = $null
    warnings = @($warnings)
    command = $command
  }
}

$report = [ordered]@{
  generated_at = (Get-Date).ToString("o")
  conversion_report_path = $ConversionReport
  ogrinfo_path = $ogrinfoPath
  inspected_count = @($entries).Count
  success_count = @($entries | Where-Object { $_.inspect_success }).Count
  failed_count = @($entries | Where-Object { -not $_.inspect_success }).Count
  entries = @($entries)
}

New-Item -ItemType Directory -Force -Path $Output | Out-Null
$reportPath = Join-Path $Output "normalized_cad_inspect_report.json"
$report | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $reportPath -Encoding UTF8
Write-Host "Normalized CAD inspect report: $reportPath"
Write-Host ("Inspected: {0}, success: {1}, failed: {2}" -f $report.inspected_count, $report.success_count, $report.failed_count)
