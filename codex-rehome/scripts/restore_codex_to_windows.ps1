$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
if ((Split-Path -Leaf $Root) -ieq "scripts") {
    $Root = Split-Path -Parent $Root
}

$Stamp = Get-Date -Format "yyyyMMdd-HHmmss"

function Backup-IfExists {
    param([string]$Path)

    if (Test-Path -LiteralPath $Path) {
        $BackupPath = "$Path.backup-$Stamp"
        Write-Host "Backing up existing data:"
        Write-Host "  $Path"
        Write-Host "  -> $BackupPath"
        Move-Item -LiteralPath $Path -Destination $BackupPath
    }
}

function Restore-Directory {
    param(
        [string]$Source,
        [string]$Destination
    )

    if (-not (Test-Path -LiteralPath $Source)) {
        Write-Host "Skipping missing source: $Source"
        return
    }

    $Parent = Split-Path -Parent $Destination
    New-Item -ItemType Directory -Force -Path $Parent | Out-Null
    Backup-IfExists -Path $Destination
    Copy-Item -LiteralPath $Source -Destination $Destination -Recurse -Force
    Write-Host "Restored: $Destination"
}

$RunningCodex = Get-Process -ErrorAction SilentlyContinue | Where-Object {
    $_.ProcessName -match "Codex"
}

if ($RunningCodex) {
    Write-Host "Codex appears to be running. Close Codex before continuing."
    Read-Host "Press Enter after Codex is closed"
}

Write-Host "Restoring Codex data..."
Write-Host "User profile: $env:USERPROFILE"
Write-Host "Roaming AppData: $env:APPDATA"
Write-Host "Local AppData: $env:LOCALAPPDATA"

Restore-Directory -Source (Join-Path $Root "home\.codex") -Destination (Join-Path $env:USERPROFILE ".codex")
Restore-Directory -Source (Join-Path $Root "appdata_roaming\Codex") -Destination (Join-Path $env:APPDATA "Codex")
Restore-Directory -Source (Join-Path $Root "appdata_roaming\com.openai.codex") -Destination (Join-Path $env:APPDATA "com.openai.codex")
Restore-Directory -Source (Join-Path $Root "appdata_roaming\OpenAI\Codex") -Destination (Join-Path $env:APPDATA "OpenAI\Codex")

Restore-Directory -Source (Join-Path $Root "appdata_local\Codex") -Destination (Join-Path $env:LOCALAPPDATA "Codex")
Restore-Directory -Source (Join-Path $Root "appdata_local\com.openai.codex") -Destination (Join-Path $env:LOCALAPPDATA "com.openai.codex")
Restore-Directory -Source (Join-Path $Root "appdata_local\com.openai.sky.CUAService") -Destination (Join-Path $env:LOCALAPPDATA "com.openai.sky.CUAService")
Restore-Directory -Source (Join-Path $Root "appdata_local\com.openai.sky.CUAService.cli") -Destination (Join-Path $env:LOCALAPPDATA "com.openai.sky.CUAService.cli")

foreach ($File in @(
    (Join-Path $env:APPDATA "Codex\SingletonLock"),
    (Join-Path $env:APPDATA "Codex\SingletonCookie"),
    (Join-Path $env:APPDATA "Codex\SingletonSocket")
)) {
    if (Test-Path -LiteralPath $File) {
        Remove-Item -LiteralPath $File -Force
    }
}

Write-Host "Done. Open Codex and log in again if prompted."

