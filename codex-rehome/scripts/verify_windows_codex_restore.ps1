param(
    [string]$PackageRoot = "",
    [switch]$Json
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($PackageRoot)) {
    $PackageRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
    if ((Split-Path -Leaf $PackageRoot) -ieq "scripts") {
        $PackageRoot = Split-Path -Parent $PackageRoot
    }
}

function Count-Files {
    param(
        [string]$Path,
        [string]$Filter = "*"
    )

    if (-not (Test-Path -LiteralPath $Path)) { return 0 }
    return @(
        Get-ChildItem -LiteralPath $Path -Filter $Filter -Recurse -Force -File -ErrorAction SilentlyContinue
    ).Count
}

function Directory-SizeMb {
    param([string]$Path)

    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    $sum = (Get-ChildItem -LiteralPath $Path -Recurse -Force -File -ErrorAction SilentlyContinue |
        Measure-Object -Property Length -Sum).Sum
    if ($null -eq $sum) { $sum = 0 }
    return [Math]::Round($sum / 1MB, 2)
}

function Path-Status {
    param([string]$Path)

    [PSCustomObject]@{
        path = $Path
        exists = Test-Path -LiteralPath $Path
        size_mb = Directory-SizeMb -Path $Path
    }
}

$CodexHome = Join-Path $env:USERPROFILE ".codex"
$RoamingCodex = Join-Path $env:APPDATA "Codex"
$RoamingComOpenAi = Join-Path $env:APPDATA "com.openai.codex"
$RoamingOpenAiCodex = Join-Path $env:APPDATA "OpenAI\Codex"

$Report = [ordered]@{
    generated_at = (Get-Date).ToString("s")
    package_root = $PackageRoot
    windows_user = $env:USERNAME
    paths = @(
        Path-Status -Path $CodexHome
        Path-Status -Path $RoamingCodex
        Path-Status -Path $RoamingComOpenAi
        Path-Status -Path $RoamingOpenAiCodex
        Path-Status -Path (Join-Path $env:LOCALAPPDATA "Codex")
    )
    counts = [ordered]@{
        sessions = Count-Files -Path (Join-Path $CodexHome "sessions") -Filter "*.jsonl"
        archived_sessions = Count-Files -Path (Join-Path $CodexHome "archived_sessions") -Filter "*.jsonl"
        skills = Count-Files -Path (Join-Path $CodexHome "skills") -Filter "SKILL.md"
        plugin_manifests = Count-Files -Path (Join-Path $CodexHome "plugins\cache") -Filter "plugin.json"
        generated_images = Count-Files -Path (Join-Path $CodexHome "generated_images")
        sqlite_files = Count-Files -Path $CodexHome -Filter "*.sqlite"
    }
    important_files = @(
        Path-Status -Path (Join-Path $CodexHome "state_5.sqlite")
        Path-Status -Path (Join-Path $CodexHome "memories_1.sqlite")
        Path-Status -Path (Join-Path $CodexHome "goals_1.sqlite")
        Path-Status -Path (Join-Path $CodexHome "config.toml")
    )
    package_files = @(
        Path-Status -Path (Join-Path $PackageRoot "MANIFEST.txt")
        Path-Status -Path (Join-Path $PackageRoot "MANIFEST.json")
        Path-Status -Path (Join-Path $PackageRoot "SHA256SUMS.txt")
        Path-Status -Path (Join-Path $PackageRoot "docs\SENSITIVE-FILES.txt")
    )
    project_candidates = @(
        Get-ChildItem -LiteralPath (Join-Path $env:USERPROFILE "Documents") -Directory -ErrorAction SilentlyContinue |
            Where-Object {
                (Test-Path -LiteralPath (Join-Path $_.FullName ".git") -PathType Container) -or
                (Test-Path -LiteralPath (Join-Path $_.FullName ".agents") -PathType Container) -or
                (Test-Path -LiteralPath (Join-Path $_.FullName "outputs") -PathType Container) -or
                (Test-Path -LiteralPath (Join-Path $_.FullName "artifacts") -PathType Container)
            } |
            Select-Object -First 50 -ExpandProperty FullName
    )
}

if ($Json) {
    $Report | ConvertTo-Json -Depth 6
    exit 0
}

Write-Host "Codex Windows restore verification"
Write-Host "Generated: $($Report.generated_at)"
Write-Host "Package root: $PackageRoot"
Write-Host ""

Write-Host "Paths:"
foreach ($item in $Report.paths) {
    $status = if ($item.exists) { "found" } else { "missing" }
    $size = if ($null -eq $item.size_mb) { "" } else { " ($($item.size_mb) MB)" }
    Write-Host "  [$status] $($item.path)$size"
}

Write-Host ""
Write-Host "Counts:"
foreach ($key in $Report.counts.Keys) {
    Write-Host "  ${key}: $($Report.counts[$key])"
}

Write-Host ""
Write-Host "Important files:"
foreach ($item in $Report.important_files) {
    $status = if ($item.exists) { "found" } else { "missing" }
    Write-Host "  [$status] $($item.path)"
}

Write-Host ""
Write-Host "Package metadata:"
foreach ($item in $Report.package_files) {
    $status = if ($item.exists) { "found" } else { "missing" }
    Write-Host "  [$status] $($item.path)"
}

Write-Host ""
Write-Host "Project candidates:"
foreach ($path in $Report.project_candidates) {
    Write-Host "  $path"
}

Write-Host ""
Write-Host "Next checks:"
Write-Host "  1. Open Codex and confirm old threads are visible."
Write-Host "  2. Reopen migrated project folders from their Windows paths."
Write-Host "  3. Reconnect GitHub, Gmail, Chrome, Feishu, or other external services if prompted."
