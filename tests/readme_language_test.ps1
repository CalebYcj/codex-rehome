$ErrorActionPreference = "Stop"

$Repo = Split-Path -Parent $PSScriptRoot
$ChinesePath = Join-Path $Repo "README.md"
$EnglishPath = Join-Path $Repo "README.en.md"

if (-not (Test-Path -LiteralPath $EnglishPath)) {
    throw "README.en.md is missing"
}

$Chinese = Get-Content -LiteralPath $ChinesePath -Raw -Encoding UTF8
$English = Get-Content -LiteralPath $EnglishPath -Raw -Encoding UTF8

if (-not $Chinese.Contains("[English](README.en.md)")) {
    throw "Chinese README does not link to README.en.md"
}
if (-not $English.Contains("](README.md)")) {
    throw "English README does not link to README.md"
}
if ($Chinese -match "## English Overview") {
    throw "Chinese README still contains the appended English overview"
}
if (-not $Chinese.Contains("## For AI Agents") -or -not $English.Contains("## For AI Agents")) {
    throw "Both READMEs must expose a concise For AI Agents section"
}
if ($Chinese.Length -ge 12000) {
    throw "Chinese README is still too long: $($Chinese.Length) characters"
}

foreach ($Phrase in @("Mac to Windows", "Windows to Mac", "Windows to Windows", "Mac to Mac", "merge-safe", "codex app")) {
    if (-not $English.Contains($Phrase)) {
        throw "English README is missing: $Phrase"
    }
}

foreach ($Phrase in @("D:\Codex-Rehome-Backup", "Red Skill", "~/.codex", "Codex Desktop")) {
    if (-not $Chinese.Contains($Phrase)) {
        throw "Chinese README is missing: $Phrase"
    }
}

foreach ($UnsafePlaceholder in @("<project>", "<restored-project-path>")) {
    if ($Chinese.Contains($UnsafePlaceholder) -or $English.Contains($UnsafePlaceholder)) {
        throw "README contains a placeholder GitHub may render as HTML: $UnsafePlaceholder"
    }
}

Write-Output "PASS bilingual README contract"
