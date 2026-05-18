param(
  [string]$TilesDir = "C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\DJB-M-SU-_\tiles"
)

$ErrorActionPreference = "Stop"
$utf8 = [System.Text.Encoding]::UTF8

function Read-UInt32LE {
  param(
    [byte[]]$Bytes,
    [int]$Offset
  )
  return [System.BitConverter]::ToUInt32($Bytes, $Offset)
}

function Write-PaddedJson {
  param(
    [byte[]]$Bytes,
    [int]$Offset,
    [int]$Length,
    [string]$Json
  )

  $jsonBytes = $utf8.GetBytes($Json)
  if ($jsonBytes.Length -gt $Length) {
    throw "新的 GLB JSON 長度 $($jsonBytes.Length) 超過原始 chunk 長度 $Length"
  }

  [System.Array]::Copy($jsonBytes, 0, $Bytes, $Offset, $jsonBytes.Length)
  for ($i = $Offset + $jsonBytes.Length; $i -lt $Offset + $Length; $i++) {
    # GLB JSON chunk 以空白 padding；維持原長度可避免重算 b3dm/glb header。
    $Bytes[$i] = 0x20
  }
}

$files = Get-ChildItem -LiteralPath $TilesDir -Filter '*.b3dm' | Sort-Object Name
$patched = 0

foreach ($file in $files) {
  $bytes = [System.IO.File]::ReadAllBytes($file.FullName)
  $magic = [System.Text.Encoding]::ASCII.GetString($bytes, 0, 4)
  if ($magic -ne "b3dm") {
    throw "$($file.Name) 不是 b3dm"
  }

  $featureJsonLen = Read-UInt32LE $bytes 12
  $featureBinLen = Read-UInt32LE $bytes 16
  $batchJsonLen = Read-UInt32LE $bytes 20
  $batchBinLen = Read-UInt32LE $bytes 24
  $glbOffset = 28 + $featureJsonLen + $featureBinLen + $batchJsonLen + $batchBinLen

  $glbMagic = [System.Text.Encoding]::ASCII.GetString($bytes, $glbOffset, 4)
  if ($glbMagic -ne "glTF") {
    throw "$($file.Name) 內含 payload 不是 GLB"
  }

  $jsonLen = Read-UInt32LE $bytes ($glbOffset + 12)
  $jsonType = [System.Text.Encoding]::ASCII.GetString($bytes, $glbOffset + 16, 4)
  if ($jsonType -ne "JSON") {
    throw "$($file.Name) 第一個 GLB chunk 不是 JSON"
  }

  $jsonOffset = $glbOffset + 20
  $jsonText = $utf8.GetString($bytes, $jsonOffset, $jsonLen).TrimEnd(" ", "`0", "`r", "`n", "`t")
  $doc = $jsonText | ConvertFrom-Json -Depth 100
  $node = $doc.nodes[0]

  if ($node.PSObject.Properties["matrix"]) {
    $node.PSObject.Properties.Remove("matrix")
    $newJson = $doc | ConvertTo-Json -Depth 100 -Compress
    Write-PaddedJson -Bytes $bytes -Offset $jsonOffset -Length $jsonLen -Json $newJson
    [System.IO.File]::WriteAllBytes($file.FullName, $bytes)
    $patched += 1
  }
}

Write-Host "patched_b3dm=$patched total=$($files.Count)"
