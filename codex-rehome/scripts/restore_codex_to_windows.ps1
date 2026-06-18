param(
    [switch]$ReplaceCodexHome,
    [switch]$ReplaceState
)

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
if ((Split-Path -Leaf $Root) -ieq "scripts") {
    $Root = Split-Path -Parent $Root
}

$Stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$SourceCodexHome = Join-Path $Root "home\.codex"
$TargetCodexHome = Join-Path $env:USERPROFILE ".codex"
$PreserveFiles = @(
    "auth.json",
    "config.toml",
    "installation_id",
    "models_cache.json",
    "chrome-native-hosts-v2.json"
)

function Write-Utf8NoBomLf {
    param(
        [Parameter(Mandatory=$true)][string]$Path,
        [Parameter(Mandatory=$true)][AllowEmptyCollection()][string[]]$Lines
    )
    $encoding = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, (($Lines -join "`n") + "`n"), $encoding)
}

function Backup-CopyIfExists {
    param([string]$Path)

    if (Test-Path -LiteralPath $Path) {
        $BackupPath = "$Path.backup-$Stamp"
        Write-Host "Backing up existing data copy:"
        Write-Host "  $Path"
        Write-Host "  -> $BackupPath"
        Copy-Item -LiteralPath $Path -Destination $BackupPath -Recurse -Force
    }
}

function Merge-Directory {
    param(
        [string]$Source,
        [string]$Destination
    )

    if (-not (Test-Path -LiteralPath $Source -PathType Container)) {
        return
    }

    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    Get-ChildItem -LiteralPath $Source -Force -ErrorAction SilentlyContinue | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $Destination $_.Name) -Recurse -Force
    }
    Write-Host "Merged: $Destination"
}

function Copy-FilePreserve {
    param(
        [string]$Source,
        [string]$Destination
    )
    if (-not (Test-Path -LiteralPath $Source -PathType Leaf)) { return }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Destination) | Out-Null
    Copy-Item -LiteralPath $Source -Destination $Destination -Force
}

function Save-PreservedFiles {
    param([string]$KeepDir)
    New-Item -ItemType Directory -Force -Path $KeepDir | Out-Null
    foreach ($name in $PreserveFiles) {
        $src = Join-Path $TargetCodexHome $name
        if (Test-Path -LiteralPath $src -PathType Leaf) {
            Copy-FilePreserve -Source $src -Destination (Join-Path $KeepDir $name)
        }
    }
}

function Restore-PreservedFiles {
    param([string]$KeepDir)
    foreach ($name in $PreserveFiles) {
        $src = Join-Path $KeepDir $name
        if (Test-Path -LiteralPath $src -PathType Leaf) {
            Copy-FilePreserve -Source $src -Destination (Join-Path $TargetCodexHome $name)
        }
    }
}

function Get-SessionEntryFromJsonl {
    param([string]$Path)

    $sessionId = ""
    $threadName = ""
    $updatedAt = ""
    $firstUser = ""

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return $null
    }

    Get-Content -LiteralPath $Path -Encoding UTF8 -ErrorAction SilentlyContinue | ForEach-Object {
        if ([string]::IsNullOrWhiteSpace($_)) { return }
        try { $row = $_ | ConvertFrom-Json -ErrorAction Stop } catch { return }
        $payload = $null
        if ($row.PSObject.Properties.Name -contains "payload") { $payload = $row.payload }

        if (($row.type -eq "session_meta") -or ($payload -and $payload.type -eq "session_meta")) {
            if ($payload -and $payload.id) { $script:sessionIdFromMeta = [string]$payload.id }
            if ($payload -and $payload.thread_name) { $script:threadNameFromMeta = [string]$payload.thread_name }
            elseif ($payload -and $payload.name) { $script:threadNameFromMeta = [string]$payload.name }
            elseif ($payload -and $payload.title) { $script:threadNameFromMeta = [string]$payload.title }
        }
        if (-not $script:sessionIdFromAny) {
            if ($row.id) { $script:sessionIdFromAny = [string]$row.id }
            elseif ($payload -and $payload.id) { $script:sessionIdFromAny = [string]$payload.id }
        }
        $ts = $null
        if ($row.timestamp) { $ts = $row.timestamp }
        elseif ($payload -and $payload.timestamp) { $ts = $payload.timestamp }
        elseif ($row.updated_at) { $ts = $row.updated_at }
        elseif ($payload -and $payload.updated_at) { $ts = $payload.updated_at }
        if ($ts) { $script:updatedAtFromRows = [string]$ts }

        if (-not $script:firstUserFromRows -and $payload -and $payload.message -and $payload.message.role -eq "user") {
            $content = $payload.message.content
            if ($content -is [string]) {
                $script:firstUserFromRows = ($content -replace "`r?`n", " ").Trim()
            }
        }
    }

    $sessionId = $script:sessionIdFromMeta
    if (-not $sessionId) { $sessionId = $script:sessionIdFromAny }
    if (-not $sessionId) { $sessionId = [System.IO.Path]::GetFileNameWithoutExtension($Path) }
    $threadName = $script:threadNameFromMeta
    if (-not $threadName) { $threadName = $script:firstUserFromRows }
    if (-not $threadName) { $threadName = [System.IO.Path]::GetFileNameWithoutExtension($Path) }
    $updatedAt = $script:updatedAtFromRows

    Remove-Variable -Scope Script -Name sessionIdFromMeta,threadNameFromMeta,sessionIdFromAny,updatedAtFromRows,firstUserFromRows -ErrorAction SilentlyContinue

    return [ordered]@{
        id = $sessionId
        thread_name = $threadName
        updated_at = $updatedAt
    }
}

function Read-SessionIndexRows {
    param([string]$Path)
    $rows = @()
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $rows }
    Get-Content -LiteralPath $Path -Encoding UTF8 -ErrorAction SilentlyContinue | ForEach-Object {
        if ([string]::IsNullOrWhiteSpace($_)) { return }
        try { $rows += ($_ | ConvertFrom-Json -ErrorAction Stop) } catch {}
    }
    return $rows
}

function Merge-SessionIndex {
    New-Item -ItemType Directory -Force -Path $TargetCodexHome | Out-Null
    $targetIndex = Join-Path $TargetCodexHome "session_index.jsonl"
    $packageIndex = Join-Path $SourceCodexHome "session_index.jsonl"
    $rows = New-Object System.Collections.Generic.List[object]
    $seen = @{}

    foreach ($row in (Read-SessionIndexRows -Path $targetIndex)) {
        if ($row.id -and -not $seen.ContainsKey([string]$row.id)) {
            $rows.Add($row)
            $seen[[string]$row.id] = $true
        }
    }

    $sourceRows = @(Read-SessionIndexRows -Path $packageIndex)
    if ($sourceRows.Count -eq 0) {
        $sourceRows = @()
        foreach ($root in @((Join-Path $SourceCodexHome "sessions"), (Join-Path $Root "selected_chats"))) {
            if (Test-Path -LiteralPath $root -PathType Container) {
                Get-ChildItem -LiteralPath $root -Filter "*.jsonl" -Recurse -Force -File -ErrorAction SilentlyContinue | ForEach-Object {
                    $entry = Get-SessionEntryFromJsonl -Path $_.FullName
                    if ($entry) { $sourceRows += [pscustomobject]$entry }
                }
            }
        }
    }

    foreach ($row in $sourceRows) {
        if ($row.id -and -not $seen.ContainsKey([string]$row.id)) {
            $rows.Add([ordered]@{
                id = [string]$row.id
                thread_name = if ($row.thread_name) { [string]$row.thread_name } else { [string]$row.id }
                updated_at = if ($row.updated_at) { [string]$row.updated_at } else { "" }
            })
            $seen[[string]$row.id] = $true
        }
    }

    $lines = foreach ($row in $rows) { $row | ConvertTo-Json -Compress -Depth 5 }
    Write-Utf8NoBomLf -Path $targetIndex -Lines $lines
    Write-Host "Merged session_index.jsonl"
}

function Merge-StateFiles {
    foreach ($pattern in @("state_*.sqlite", "state_*.sqlite-*", "memories_*.sqlite", "memories_*.sqlite-*", "goals_*.sqlite", "goals_*.sqlite-*")) {
        Get-ChildItem -LiteralPath $SourceCodexHome -Filter $pattern -File -Force -ErrorAction SilentlyContinue | ForEach-Object {
            $dst = Join-Path $TargetCodexHome $_.Name
            if ($ReplaceState -or -not (Test-Path -LiteralPath $dst)) {
                Copy-FilePreserve -Source $_.FullName -Destination $dst
                Write-Host "Restored state file: $dst"
            } else {
                Write-Host "Kept existing state file: $dst"
            }
        }
    }
}

function Replace-CodexHome {
    $keepDir = Join-Path $env:USERPROFILE ".codex.preserved-$Stamp"
    Save-PreservedFiles -KeepDir $keepDir
    if (Test-Path -LiteralPath $TargetCodexHome) {
        $backup = "$TargetCodexHome.backup-$Stamp"
        Write-Host "Replacing .codex after backup:"
        Write-Host "  $TargetCodexHome"
        Write-Host "  -> $backup"
        Move-Item -LiteralPath $TargetCodexHome -Destination $backup
    }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $TargetCodexHome) | Out-Null
    Copy-Item -LiteralPath $SourceCodexHome -Destination $TargetCodexHome -Recurse -Force
    Restore-PreservedFiles -KeepDir $keepDir
    Remove-Item -LiteralPath $keepDir -Recurse -Force -ErrorAction SilentlyContinue
}

function Merge-CodexHome {
    New-Item -ItemType Directory -Force -Path $TargetCodexHome | Out-Null
    Backup-CopyIfExists -Path $TargetCodexHome

    Merge-Directory -Source (Join-Path $SourceCodexHome "sessions") -Destination (Join-Path $TargetCodexHome "sessions")
    Merge-Directory -Source (Join-Path $SourceCodexHome "archived_sessions") -Destination (Join-Path $TargetCodexHome "archived_sessions")
    Merge-Directory -Source (Join-Path $SourceCodexHome "skills") -Destination (Join-Path $TargetCodexHome "skills")
    Merge-Directory -Source (Join-Path $SourceCodexHome "plugins\cache") -Destination (Join-Path $TargetCodexHome "plugins\cache")
    Merge-Directory -Source (Join-Path $SourceCodexHome "generated_images") -Destination (Join-Path $TargetCodexHome "generated_images")

    Merge-StateFiles
    Merge-SessionIndex

    foreach ($name in $PreserveFiles) {
        $dst = Join-Path $TargetCodexHome $name
        if (Test-Path -LiteralPath $dst -PathType Leaf) {
            Write-Host "Preserved target file: $dst"
        }
    }
}

if (-not (Test-Path -LiteralPath $SourceCodexHome -PathType Container)) {
    throw "Required source missing: $SourceCodexHome"
}

$RunningCodex = Get-Process -ErrorAction SilentlyContinue | Where-Object {
    $_.ProcessName -match "Codex"
}

if ($RunningCodex) {
    Write-Host "Codex appears to be running. Close Codex before continuing."
    Read-Host "Press Enter after Codex is closed"
}

Write-Host "Restoring Codex data..."
Write-Host "Restore mode: $(if ($ReplaceCodexHome) { 'replace-codex-home' } else { 'merge' })"
Write-Host "User profile: $env:USERPROFILE"
Write-Host "Roaming AppData: $env:APPDATA"
Write-Host "Local AppData: $env:LOCALAPPDATA"

if ($ReplaceCodexHome) {
    Replace-CodexHome
} else {
    Merge-CodexHome
}

foreach ($pair in @(
    @((Join-Path $Root "appdata_roaming\Codex"), (Join-Path $env:APPDATA "Codex")),
    @((Join-Path $Root "appdata_roaming\com.openai.codex"), (Join-Path $env:APPDATA "com.openai.codex")),
    @((Join-Path $Root "appdata_roaming\OpenAI\Codex"), (Join-Path $env:APPDATA "OpenAI\Codex")),
    @((Join-Path $Root "appdata_local\Codex"), (Join-Path $env:LOCALAPPDATA "Codex")),
    @((Join-Path $Root "appdata_local\com.openai.codex"), (Join-Path $env:LOCALAPPDATA "com.openai.codex")),
    @((Join-Path $Root "appdata_local\com.openai.sky.CUAService"), (Join-Path $env:LOCALAPPDATA "com.openai.sky.CUAService")),
    @((Join-Path $Root "appdata_local\com.openai.sky.CUAService.cli"), (Join-Path $env:LOCALAPPDATA "com.openai.sky.CUAService.cli"))
)) {
    $src = $pair[0]
    $dst = $pair[1]
    if (Test-Path -LiteralPath $src -PathType Container) {
        Backup-CopyIfExists -Path $dst
        Merge-Directory -Source $src -Destination $dst
    } else {
        Write-Host "Skipping missing source: $src"
    }
}

foreach ($File in @(
    (Join-Path $env:APPDATA "Codex\SingletonLock"),
    (Join-Path $env:APPDATA "Codex\SingletonCookie"),
    (Join-Path $env:APPDATA "Codex\SingletonSocket")
)) {
    if (Test-Path -LiteralPath $File) {
        Remove-Item -LiteralPath $File -Force
    }
}

Write-Host "Done. Merge restore completed. Open Codex and log in again if prompted."
