param(
  [string]$ProjectRoot = "C:\Users\stw_s\Desktop\ifc_to_3dtiles"
)

$ErrorActionPreference = "Stop"

# Guardrail: production Rust 只能做通用 pipeline，不得把個案 layer/source 語意寫死。
# 允許個案詞出現在 tests、docs、以及本機覆寫 config/semantic_rules.local.json。
$forbiddenTerms = @(
  "淡江",
  "主橋",
  "管理中心",
  "電梯",
  "監測",
  "P130"
)

$srcRoot = Join-Path $ProjectRoot "src"
if (-not (Test-Path -LiteralPath $srcRoot)) {
  throw "找不到 src 目錄：$srcRoot"
}

$violations = New-Object System.Collections.Generic.List[string]

# src/**/*.rs
$files = Get-ChildItem -LiteralPath $srcRoot -Recurse -File -Filter "*.rs"
foreach ($file in $files) {
  $content = Get-Content -LiteralPath $file.FullName -Raw
  foreach ($term in $forbiddenTerms) {
    if ($content.Contains($term)) {
      $relative = [System.IO.Path]::GetRelativePath($ProjectRoot, $file.FullName)
      $matches = Select-String -LiteralPath $file.FullName -Pattern $term -SimpleMatch
      foreach ($match in $matches) {
        $violations.Add("${relative}:$($match.LineNumber): forbidden project semantic term '$term'")
      }
    }
  }
}

if ($violations.Count -gt 0) {
  Write-Error ("Semantic guardrail failed. Move project-specific rules to config/semantic_rules.local.json:`n" + ($violations -join "`n"))
  exit 1
}

Write-Host "semantic guardrail passed"
