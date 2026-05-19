param(
    [string]$PublishDir = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\inspect_tamkang\publish",
    [int]$Port = 8120,
    [switch]$Open,
    [switch]$Stop
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $PublishDir)) {
    throw "找不到 Phase 1F publish 目錄：$PublishDir"
}

$pidFile = Join-Path $env:TEMP "ifc_phase1f_publish_viewer_$Port.pid"

if ($Stop) {
    if (Test-Path -LiteralPath $pidFile) {
        $pidValue = Get-Content -LiteralPath $pidFile -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($pidValue) {
            Stop-Process -Id ([int]$pidValue) -ErrorAction SilentlyContinue
            Write-Host "[Phase 1F Viewer] stopped pid=$pidValue"
        }
        Remove-Item -LiteralPath $pidFile -ErrorAction SilentlyContinue
    } else {
        Write-Host "[Phase 1F Viewer] no pid file: $pidFile"
    }
    exit 0
}

foreach ($name in @("index.html", "sources_manifest.json", "debug_overlays.json")) {
    $path = Join-Path $PublishDir $name
    if (-not (Test-Path -LiteralPath $path)) {
        throw "Phase 1F viewer 缺少檔案：$path"
    }
}

$pythonCommand = Get-Command python -ErrorAction SilentlyContinue
if ($pythonCommand) {
    $pythonExe = $pythonCommand.Source
} elseif (Test-Path -LiteralPath "C:\Python312_64\python.exe") {
    $pythonExe = "C:\Python312_64\python.exe"
} else {
    throw "找不到 python，無法啟動本機 HTTP server"
}

$existing = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue
if (-not $existing) {
    $process = Start-Process -FilePath $pythonExe -ArgumentList @(
        "-m",
        "http.server",
        [string]$Port,
        "--bind",
        "127.0.0.1",
        "--directory",
        $PublishDir
    ) -WindowStyle Hidden -PassThru
    $process.Id | Set-Content -LiteralPath $pidFile
    Start-Sleep -Milliseconds 800
    Write-Host "[Phase 1F Viewer] started pid=$($process.Id)"
} else {
    Write-Host "[Phase 1F Viewer] already listening on port $Port"
}

$url = "http://127.0.0.1:$Port/index.html"
$response = Invoke-WebRequest -Uri $url -UseBasicParsing -TimeoutSec 10
if ($response.StatusCode -ne 200) {
    throw "Phase 1F viewer HTTP check failed: $($response.StatusCode)"
}

Write-Host "[Phase 1F Viewer] $url"
Write-Host "[Phase 1F Viewer] IIS/ASP.NET 401.3 可忽略；請用上面的 127.0.0.1 URL 開。"

if ($Open) {
    Start-Process $url
}
