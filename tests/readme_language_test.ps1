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
foreach ($Phrase in @("Windows to Windows", "Windows to macOS", "macOS to Windows", "macOS to macOS", "ReHome Desktop")) {
    if (-not $English.Contains($Phrase)) {
        throw "English README is missing: $Phrase"
    }
}

foreach ($Phrase in @(".rehome", "Codex ReHome Skill", "Codex Desktop", "ReHome Desktop")) {
    if (-not $Chinese.Contains($Phrase)) {
        throw "Chinese README is missing: $Phrase"
    }
}

foreach ($Readme in @($Chinese, $English)) {
    if (-not $Readme.Contains("docs/desktop-install.md") -and -not $Readme.Contains("docs/desktop-install.en.md")) {
        throw "README does not link to its ReHome Desktop installation guide"
    }
    if (-not $Readme.Contains("https://github.com/CalebYcj/codex-rehome/releases")) {
        throw "README does not link to GitHub Releases"
    }
}

foreach ($Phrase in @("ReHome Core", "Codex Bridge")) {
    if (-not $Chinese.Contains($Phrase) -or -not $English.Contains($Phrase)) {
        throw "Both READMEs must state the bundled component: $Phrase"
    }
}

Write-Output "PASS bilingual README contract"
