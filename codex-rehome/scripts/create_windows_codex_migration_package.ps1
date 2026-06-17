param(
    [ValidateSet("standard", "full", "full-with-secrets")]
    [string]$Mode = "standard",
    [string]$Out = (Join-Path $env:USERPROFILE "Desktop"),
    [string[]]$Project = @(),
    [switch]$IUnderstandSecrets
)

$ErrorActionPreference = "Stop"

if ($Mode -eq "full-with-secrets" -and -not $IUnderstandSecrets) {
    throw "Refusing full-with-secrets without -IUnderstandSecrets. This mode may package auth tokens, .env files, browser login state, and private keys."
}

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$Stage = Join-Path $Out "Codex-Migration-Windows-Source-$Stamp"
$ZipPath = "$Stage.zip"
$Docs = Join-Path $Stage "docs"

New-Item -ItemType Directory -Force -Path `
    (Join-Path $Stage "home"), `
    (Join-Path $Stage "appdata_roaming\OpenAI"), `
    (Join-Path $Stage "appdata_local"), `
    (Join-Path $Stage "mac_only\Library\Preferences"), `
    (Join-Path $Stage "projects"), `
    $Docs | Out-Null

$AlwaysSkipNames = @(
    ".DS_Store", ".tmp", "tmp", "process_manager", "vendor_imports",
    ".git", "node_modules", ".venv", "venv", "__pycache__",
    "SingletonLock", "SingletonCookie", "SingletonSocket", "RunningChromeVersion"
)
$AlwaysSkipExtensions = @(".ipc", ".sock")
$SecretNames = @(
    "auth.json", "Cookies", "Cookies-journal", "Login Data", "Login Data For Account",
    "Login Data-journal", "Login Data For Account-journal", "Local Storage", "Session Storage",
    ".env", "id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"
)
$SecretExtensions = @(".pem", ".key")
$StandardSkipNames = @("logs", "Cache", "Caches", "GPUCache", "Code Cache", "CacheStorage")

function Should-Skip {
    param([System.IO.FileSystemInfo]$Item)

    if ($AlwaysSkipNames -contains $Item.Name) { return $true }
    if ($AlwaysSkipExtensions -contains $Item.Extension) { return $true }

    if ($Mode -ne "full-with-secrets") {
        if ($SecretNames -contains $Item.Name) { return $true }
        if ($Item.Name -like ".env.*") { return $true }
        if ($SecretExtensions -contains $Item.Extension) { return $true }
    }

    if ($Mode -eq "standard") {
        if ($StandardSkipNames -ccontains $Item.Name) { return $true }
        if ($Item.Name -like "logs_*.sqlite*") { return $true }
    }

    return $false
}

function Copy-DirectoryFiltered {
    param(
        [string]$Source,
        [string]$Destination
    )

    if (-not (Test-Path -LiteralPath $Source -PathType Container)) { return }
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null

    Get-ChildItem -LiteralPath $Source -Force -ErrorAction SilentlyContinue | ForEach-Object {
        if (Should-Skip -Item $_) { return }
        $target = Join-Path $Destination $_.Name
        if ($_.PSIsContainer) {
            Copy-DirectoryFiltered -Source $_.FullName -Destination $target
        } else {
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
            Copy-Item -LiteralPath $_.FullName -Destination $target -Force
        }
    }
}

function Copy-IfDirectory {
    param([string]$Source, [string]$Destination)
    if (Test-Path -LiteralPath $Source -PathType Container) {
        Copy-DirectoryFiltered -Source $Source -Destination $Destination
    }
}

Copy-IfDirectory (Join-Path $env:USERPROFILE ".codex") (Join-Path $Stage "home\.codex")
Copy-IfDirectory (Join-Path $env:APPDATA "Codex") (Join-Path $Stage "appdata_roaming\Codex")
Copy-IfDirectory (Join-Path $env:APPDATA "com.openai.codex") (Join-Path $Stage "appdata_roaming\com.openai.codex")
Copy-IfDirectory (Join-Path $env:APPDATA "OpenAI\Codex") (Join-Path $Stage "appdata_roaming\OpenAI\Codex")

if ($Mode -ne "standard") {
    Copy-IfDirectory (Join-Path $env:LOCALAPPDATA "Codex") (Join-Path $Stage "appdata_local\Codex")
    Copy-IfDirectory (Join-Path $env:LOCALAPPDATA "com.openai.codex") (Join-Path $Stage "appdata_local\com.openai.codex")
    Copy-IfDirectory (Join-Path $env:LOCALAPPDATA "com.openai.sky.CUAService") (Join-Path $Stage "appdata_local\com.openai.sky.CUAService")
    Copy-IfDirectory (Join-Path $env:LOCALAPPDATA "com.openai.sky.CUAService.cli") (Join-Path $Stage "appdata_local\com.openai.sky.CUAService.cli")
}

foreach ($projectPath in $Project) {
    if (Test-Path -LiteralPath $projectPath -PathType Container) {
        Copy-DirectoryFiltered -Source $projectPath -Destination (Join-Path $Stage "projects\$(Split-Path -Leaf $projectPath)")
    } else {
        Write-Warning "Missing project: $projectPath"
    }
}

$SensitiveReport = Join-Path $Docs "SENSITIVE-FILES.txt"
$sensitivePaths = @(
    (Join-Path $env:USERPROFILE ".codex\auth.json"),
    (Join-Path $env:APPDATA "Codex\Cookies"),
    (Join-Path $env:APPDATA "Codex\Default\Login Data"),
    (Join-Path $env:APPDATA "Codex\Local Storage"),
    (Join-Path $env:APPDATA "Codex\Session Storage")
) | Where-Object { Test-Path -LiteralPath $_ }

@(
    "Sensitive files report"
    "Generated: $Stamp"
    "Mode: $Mode"
    ""
    "The following paths exist or matched common sensitive patterns on the source Windows machine."
    "Contents are intentionally not printed."
    ""
    $sensitivePaths
) | Set-Content -LiteralPath $SensitiveReport -Encoding UTF8

if ($Mode -ne "standard") {
    $envReport = Join-Path $Docs "ENV-INVENTORY.txt"
    $toolLines = foreach ($cmd in @("codex", "git", "node", "npm", "pnpm", "yarn", "python", "py", "uv", "cargo", "rustc", "go")) {
        $found = Get-Command $cmd -ErrorAction SilentlyContinue
        if ($found) {
            try { "${cmd}: $(& $cmd --version 2>$null | Select-Object -First 1)" } catch { "${cmd}: $($found.Source)" }
        }
    }
    @(
        "Environment inventory"
        "Generated: $Stamp"
        ""
        "[system]"
        "Computer=$env:COMPUTERNAME"
        "User=$env:USERNAME"
        "OS=$([System.Environment]::OSVersion.VersionString)"
        ""
        "[tools]"
        $toolLines
    ) | Set-Content -LiteralPath $envReport -Encoding UTF8
}

$readme = @"
Codex migration package
=======================

This package can be restored to either Windows or Mac.

Windows restore:
1. Install Codex, open it once, then close it.
2. Run in PowerShell:
   Set-ExecutionPolicy -Scope Process Bypass
   .\Restore-Codex-To-Windows.ps1
3. Verify:
   .\Verify-Codex-Windows-Restore.ps1

Mac restore:
1. Install Codex, open it once, then close it.
2. Run in Terminal:
   bash Restore-Codex-To-Mac.sh
3. Verify:
   bash Verify-Codex-Mac-Restore.sh

Project folders, if included, are under projects\. Move them to your desired project location and reopen the folder in Codex.

By default this package excludes browser login state, auth.json, .env files, and private keys. If it was created with full-with-secrets, treat it like a password vault.
"@
$readme | Set-Content -LiteralPath (Join-Path $Stage "README-Restore.txt") -Encoding UTF8

foreach ($pair in @(
    @("restore_codex_to_windows.ps1", "Restore-Codex-To-Windows.ps1"),
    @("verify_windows_codex_restore.ps1", "Verify-Codex-Windows-Restore.ps1"),
    @("restore_codex_to_mac.sh", "Restore-Codex-To-Mac.sh"),
    @("verify_mac_codex_restore.sh", "Verify-Codex-Mac-Restore.sh"),
    @("collect_mac_codex_inventory.sh", "Collect-Mac-Codex-Inventory.sh")
)) {
    $src = Join-Path $ScriptDir $pair[0]
    if (Test-Path -LiteralPath $src) {
        Copy-Item -LiteralPath $src -Destination (Join-Path $Stage $pair[1]) -Force
    }
}

$manifestText = @()
$manifestText += "created_at=$Stamp"
$manifestText += "source_os=Windows"
$manifestText += "source_home=$env:USERPROFILE"
$manifestText += "mode=$Mode"
$manifestText += "package=$ZipPath"
$manifestText += "projects=$($Project -join ' ')"
$manifestText += ""
$manifestText += "[counts]"
$codexHome = Join-Path $Stage "home\.codex"
$manifestText += "sessions=$(@(Get-ChildItem -LiteralPath (Join-Path $codexHome "sessions") -Filter "*.jsonl" -Recurse -Force -File -ErrorAction SilentlyContinue).Count)"
$manifestText += "archived_sessions=$(@(Get-ChildItem -LiteralPath (Join-Path $codexHome "archived_sessions") -Filter "*.jsonl" -Recurse -Force -File -ErrorAction SilentlyContinue).Count)"
$manifestText += "skills=$(@(Get-ChildItem -LiteralPath (Join-Path $codexHome "skills") -Filter "SKILL.md" -Recurse -Force -File -ErrorAction SilentlyContinue).Count)"
$manifestText += "plugin_manifests=$(@(Get-ChildItem -LiteralPath (Join-Path $codexHome "plugins\cache") -Filter "plugin.json" -Recurse -Force -File -ErrorAction SilentlyContinue).Count)"
$manifestText | Set-Content -LiteralPath (Join-Path $Stage "MANIFEST.txt") -Encoding UTF8

@{
    created_at = $Stamp
    source_os = "Windows"
    source_home = $env:USERPROFILE
    mode = $Mode
    package = $ZipPath
    projects = $Project
    notes = @(
        "Path mappings are recorded rather than applied to JSONL sessions in place.",
        "Use docs/SENSITIVE-FILES.txt to review suspected sensitive files without exposing values.",
        "Run the restore script for the target OS only after closing Codex on that target."
    )
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $Stage "MANIFEST.json") -Encoding UTF8

Get-ChildItem -LiteralPath $Stage -Recurse -Force -File |
    ForEach-Object {
        $rel = Resolve-Path -LiteralPath $_.FullName -Relative
        "$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLower())  $rel"
    } | Set-Content -LiteralPath (Join-Path $Stage "SHA256SUMS.txt") -Encoding UTF8

if (Test-Path -LiteralPath $ZipPath) { Remove-Item -LiteralPath $ZipPath -Force }
Compress-Archive -LiteralPath $Stage -DestinationPath $ZipPath -Force

Write-Host "Created: $ZipPath"
Write-Host "Stage: $Stage"
