param(
    [ValidateSet("standard", "full", "full-with-secrets")]
    [string]$Mode = "standard",
    [string]$Out = (Join-Path $env:USERPROFILE "Desktop"),
    [string[]]$Project = @(),
    [string[]]$SelectedChat = @(),
    [switch]$IUnderstandSecrets
)

$ErrorActionPreference = "Stop"

$PackageSchemaVersion = "3"

if ($Mode -eq "full-with-secrets" -and -not $IUnderstandSecrets) {
    throw "Refusing full-with-secrets without -IUnderstandSecrets. This mode may package auth tokens, .env files, browser login state, and private keys."
}

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$Stage = Join-Path $Out "Codex-Migration-Windows-Source-$Stamp"
$ZipPath = "$Stage.zip"
$Docs = Join-Path $Stage "docs"
$Metadata = Join-Path $Stage "metadata"

New-Item -ItemType Directory -Force -Path `
    (Join-Path $Stage "home"), `
    (Join-Path $Stage "appdata_roaming\OpenAI"), `
    (Join-Path $Stage "appdata_local"), `
    (Join-Path $Stage "mac_only\Library\Preferences"), `
    (Join-Path $Stage "projects"), `
    (Join-Path $Stage "selected_chats"), `
    $Metadata, `
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
$ExcludeSummary = @(
    "always_excluded=.DS_Store,.tmp,tmp,process_manager,vendor_imports,.git,node_modules,.venv,venv,__pycache__,Singleton runtime files,*.ipc,*.sock",
    "standard_excluded=logs,logs_*.sqlite*,Cache,Caches,GPUCache,Code Cache,CacheStorage",
    "secrets_excluded_unless_full_with_secrets=auth.json,Cookies,Login Data,Local Storage,Session Storage,.env,.env.*,private keys,*.pem,*.key"
)

function Write-Utf8NoBomLf {
    param(
        [Parameter(Mandatory=$true)][string]$Path,
        [Parameter(Mandatory=$true)][AllowEmptyCollection()][AllowEmptyString()][string[]]$Lines
    )
    $encoding = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, (($Lines -join "`n") + "`n"), $encoding)
}

function Write-RawUtf8NoBomLf {
    param(
        [Parameter(Mandatory=$true)][string]$Path,
        [Parameter(Mandatory=$true)][string]$Text
    )
    $encoding = New-Object System.Text.UTF8Encoding($false)
    $normalized = $Text -replace "`r`n", "`n" -replace "`r", "`n"
    if (-not $normalized.EndsWith("`n")) { $normalized += "`n" }
    [System.IO.File]::WriteAllText($Path, $normalized, $encoding)
}

function Find-PythonForSqlite {
    $candidates = @(
        @("python"),
        @("python3"),
        @("py", "-3")
    )
    foreach ($candidate in $candidates) {
        $cmd = $candidate[0]
        $extra = @()
        if ($candidate.Count -gt 1) { $extra = $candidate[1..($candidate.Count - 1)] }
        $found = Get-Command $cmd -ErrorAction SilentlyContinue
        if (-not $found) { continue }
        try {
            & $cmd @extra -c "import json, sqlite3" 2>$null
            if ($LASTEXITCODE -eq 0) {
                return @{ command = $cmd; args = $extra }
            }
        } catch {}
    }
    return $null
}

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

function Count-Files {
    param([string]$Path, [string]$Filter = "*")
    if (-not (Test-Path -LiteralPath $Path)) { return 0 }
    return @(Get-ChildItem -LiteralPath $Path -Filter $Filter -Recurse -Force -File -ErrorAction SilentlyContinue).Count
}

function Count-ImmediateDirectories {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return 0 }
    return @(Get-ChildItem -LiteralPath $Path -Directory -Force -ErrorAction SilentlyContinue).Count
}

function Count-JsonlEntries {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return 0 }
    return @([System.IO.File]::ReadLines($Path) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }).Count
}

function Read-JsonlRows {
    param([string]$Path)
    $rows = @()
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $rows }
    Get-Content -LiteralPath $Path -Encoding UTF8 -ErrorAction SilentlyContinue | ForEach-Object {
        if ([string]::IsNullOrWhiteSpace($_)) { return }
        try { $rows += ($_ | ConvertFrom-Json -ErrorAction Stop) } catch {}
    }
    return $rows
}

function Get-SessionEntryFromJsonl {
    param([string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $null }

    $sessionId = ""
    $threadName = ""
    $updatedAt = ""
    $firstUser = ""

    foreach ($line in [System.IO.File]::ReadLines($Path)) {
        if ([string]::IsNullOrWhiteSpace($line)) { continue }
        try { $row = $line | ConvertFrom-Json -ErrorAction Stop } catch { continue }
        $payload = $null
        if ($row.PSObject.Properties.Name -contains "payload") { $payload = $row.payload }

        if (($row.type -eq "session_meta") -or ($payload -and $payload.type -eq "session_meta")) {
            if ($payload -and $payload.id) { $sessionId = [string]$payload.id }
            elseif ($row.id) { $sessionId = [string]$row.id }

            if ($payload -and $payload.thread_name) { $threadName = [string]$payload.thread_name }
            elseif ($payload -and $payload.name) { $threadName = [string]$payload.name }
            elseif ($payload -and $payload.title) { $threadName = [string]$payload.title }
        }

        if (-not $sessionId) {
            if ($row.id) { $sessionId = [string]$row.id }
            elseif ($payload -and $payload.id) { $sessionId = [string]$payload.id }
        }

        if ($row.timestamp) { $updatedAt = [string]$row.timestamp }
        elseif ($payload -and $payload.timestamp) { $updatedAt = [string]$payload.timestamp }
        elseif ($row.updated_at) { $updatedAt = [string]$row.updated_at }
        elseif ($payload -and $payload.updated_at) { $updatedAt = [string]$payload.updated_at }

        if (-not $firstUser -and $payload -and $payload.message -and $payload.message.role -eq "user") {
            $content = $payload.message.content
            if ($content -is [string]) {
                $firstUser = (($content -replace "`r?`n", " ").Trim())
                if ($firstUser.Length -gt 80) { $firstUser = $firstUser.Substring(0, 80) }
            }
        }
    }

    if (-not $sessionId) { $sessionId = [System.IO.Path]::GetFileNameWithoutExtension($Path) }
    if (-not $threadName) { $threadName = if ($firstUser) { $firstUser } else { [System.IO.Path]::GetFileNameWithoutExtension($Path) } }

    return [ordered]@{
        id = $sessionId
        thread_name = $threadName
        updated_at = $updatedAt
    }
}

function Ensure-SelectedChatInSessions {
    param([string]$ChatPath)

    $sessionsRoot = Join-Path $Stage "home\.codex\sessions"
    New-Item -ItemType Directory -Force -Path $sessionsRoot | Out-Null

    $resolvedChat = (Resolve-Path -LiteralPath $ChatPath).Path
    $sourceSessions = Join-Path $env:USERPROFILE ".codex\sessions"
    $alreadyInSourceSessions = $false
    if (Test-Path -LiteralPath $sourceSessions -PathType Container) {
        $sourceSessionsFull = (Resolve-Path -LiteralPath $sourceSessions).Path
        $alreadyInSourceSessions = $resolvedChat.StartsWith($sourceSessionsFull, [System.StringComparison]::OrdinalIgnoreCase)
    }

    if (-not $alreadyInSourceSessions) {
        $target = Join-Path $sessionsRoot ("selected_chats\" + (Split-Path -Leaf $ChatPath))
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
        Copy-Item -LiteralPath $ChatPath -Destination $target -Force
    }
}

function Ensure-SessionIndex {
    $codexHome = Join-Path $Stage "home\.codex"
    $sessionsRoot = Join-Path $codexHome "sessions"
    $indexPath = Join-Path $codexHome "session_index.jsonl"
    New-Item -ItemType Directory -Force -Path $codexHome | Out-Null

    $rows = New-Object System.Collections.Generic.List[object]
    $seen = @{}

    foreach ($row in (Read-JsonlRows -Path $indexPath)) {
        if ($row.id -and -not $seen.ContainsKey([string]$row.id)) {
            $rows.Add($row)
            $seen[[string]$row.id] = $true
        }
    }

    foreach ($root in @($sessionsRoot, (Join-Path $Stage "selected_chats"))) {
        if (-not (Test-Path -LiteralPath $root -PathType Container)) { continue }
        Get-ChildItem -LiteralPath $root -Filter "*.jsonl" -Recurse -Force -File -ErrorAction SilentlyContinue | ForEach-Object {
            $entry = Get-SessionEntryFromJsonl -Path $_.FullName
            if ($entry -and $entry.id -and -not $seen.ContainsKey([string]$entry.id)) {
                $rows.Add([ordered]@{
                    id = [string]$entry.id
                    thread_name = [string]$entry.thread_name
                    updated_at = [string]$entry.updated_at
                })
                $seen[[string]$entry.id] = $true
            }
        }
    }

    $lines = foreach ($row in $rows) { $row | ConvertTo-Json -Compress -Depth 5 }
    Write-Utf8NoBomLf -Path $indexPath -Lines $lines
}

function Export-UiReadyMetadata {
    $requestPath = Join-Path $Metadata "export_request.json"
    $stateFiles = @()
    $sourceCodexHome = Join-Path $env:USERPROFILE ".codex"
    if (Test-Path -LiteralPath $sourceCodexHome -PathType Container) {
        $stateFiles = @(Get-ChildItem -LiteralPath $sourceCodexHome -Filter "state_*.sqlite" -Force -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | ForEach-Object { $_.FullName })
    }

    $request = [ordered]@{
        created_at = $Stamp
        source_os = "Windows"
        source_home = $env:USERPROFILE
        source_codex_home = $sourceCodexHome
        stage = $Stage
        metadata_dir = $Metadata
        projects = $Project
        selected_chats = $SelectedChat
        state_files = $stateFiles
        global_state = (Join-Path $sourceCodexHome ".codex-global-state.json")
    }
    Write-RawUtf8NoBomLf -Path $requestPath -Text ($request | ConvertTo-Json -Depth 8)

    $python = Find-PythonForSqlite
    if (-not $python) {
        Write-Warning "Python with sqlite3 was not found. UI-ready metadata export skipped."
        return
    }

    $pyCode = @'
import json
import os
import re
import sqlite3
import sys
from pathlib import Path, PureWindowsPath

request_path = Path(sys.argv[1])
req = json.loads(request_path.read_text(encoding="utf-8"))
metadata_dir = Path(req["metadata_dir"])
metadata_dir.mkdir(parents=True, exist_ok=True)
source_home = req.get("source_home") or ""
source_codex_home = Path(req.get("source_codex_home") or "")

def win_norm(path):
    if not path:
        return ""
    s = str(path).replace("/", "\\")
    if s.startswith("\\\\?\\"):
        s = s[4:]
    return s.rstrip("\\").lower()

def variants(path):
    vals = []
    if not path:
        return vals
    s = str(path)
    vals.append(s)
    if s.startswith("\\\\?\\"):
        vals.append(s[4:])
    if not s.startswith("\\\\?\\"):
        vals.append("\\\\?\\" + s)
    vals.append(s.replace("\\", "/"))
    if s.startswith("\\\\?\\"):
        vals.append(s[4:].replace("\\", "/"))
    vals.append("/" + s)
    return list(dict.fromkeys(vals))

def selected_id(path):
    sid = ""
    try:
        with open(path, "r", encoding="utf-8", errors="ignore") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    row = json.loads(line)
                except Exception:
                    continue
                payload = row.get("payload") if isinstance(row.get("payload"), dict) else {}
                if row.get("type") == "session_meta" or payload.get("type") == "session_meta":
                    sid = str(payload.get("id") or row.get("id") or sid)
                    if sid:
                        return sid
                if not sid:
                    sid = str(row.get("id") or payload.get("id") or "")
    except Exception:
        pass
    if sid:
        return sid
    match = re.search(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}", Path(path).stem, re.I)
    return match.group(0) if match else Path(path).stem

project_entries = []
project_roots = set()
for project in req.get("projects") or []:
    if not project:
        continue
    source_path = str(Path(project))
    name = Path(source_path).name
    entry = {
        "source_path": source_path,
        "source_path_normalized": win_norm(source_path),
        "source_path_variants": variants(source_path),
        "package_project_name": name,
        "package_project_path": f"projects/{name}",
        "target_mac_default_path": f"~/Documents/Codex-Restored-Projects/{name}"
    }
    project_entries.append(entry)
    project_roots.add(win_norm(source_path))

selected_ids = []
selected_chat_files = []
for chat in req.get("selected_chats") or []:
    if not chat:
        continue
    sid = selected_id(chat)
    selected_ids.append(sid)
    selected_chat_files.append({"id": sid, "source_path": chat, "package_path": f"selected_chats/{Path(chat).name}"})

def path_matches_project(path):
    n = win_norm(path)
    if not n:
        return False
    return any(n == root or n.startswith(root + "\\") for root in project_roots)

thread_rows = []
seen = set()
state_files = [p for p in req.get("state_files") or [] if p and Path(p).exists()]
for state_file in state_files:
    try:
        con = sqlite3.connect(f"file:{Path(state_file).as_posix()}?mode=ro", uri=True)
        con.row_factory = sqlite3.Row
        cols = [r[1] for r in con.execute("pragma table_info(threads)").fetchall()]
        order_col = "updated_at_ms" if "updated_at_ms" in cols else "updated_at"
        rows = con.execute(f"select * from threads order by {order_col} desc").fetchall()
    except Exception:
        continue
    for row in rows:
        d = {k: row[k] for k in row.keys()}
        tid = str(d.get("id") or "")
        if not tid or tid in seen:
            continue
        include = tid in selected_ids or path_matches_project(d.get("cwd"))
        if not include and not selected_ids and not project_roots and len(thread_rows) < 50:
            include = True
        if not include:
            continue
        rollout_path = d.get("rollout_path") or ""
        rel = ""
        try:
            rp = Path(str(rollout_path))
            ch = source_codex_home
            rel_to_codex = rp.relative_to(ch)
            rel = "home/.codex/" + rel_to_codex.as_posix()
        except Exception:
            rel = ""
        d["source_state_file"] = str(state_file)
        d["relative_package_session_path"] = rel
        thread_rows.append(d)
        seen.add(tid)
    try:
        con.close()
    except Exception:
        pass

for row in thread_rows:
    cwd = row.get("cwd") or ""
    if not cwd:
        continue
    cwd_name = PureWindowsPath(str(cwd).replace("\\\\?\\", "")).name
    for entry in project_entries:
        if entry.get("package_project_name") == cwd_name:
            existing = set(entry.get("source_path_variants") or [])
            for value in variants(cwd):
                if value and value not in existing:
                    entry.setdefault("source_path_variants", []).append(value)
                    existing.add(value)
            entry.setdefault("additional_source_paths", [])
            if cwd not in entry["additional_source_paths"]:
                entry["additional_source_paths"].append(cwd)
            project_roots.add(win_norm(cwd))

thread_ids = {str(r.get("id")) for r in thread_rows if r.get("id")}
global_state_path = Path(req.get("global_state") or "")
registry = {
    "electron-saved-workspace-roots": [],
    "project-order": [],
    "active-workspace-roots": [],
    "projectless-thread-ids": [],
    "thread-workspace-root-hints": {},
    "thread-projectless-output-directories": {},
    "heartbeat-thread-permissions-by-id": {}
}
if global_state_path.exists():
    try:
        gs = json.loads(global_state_path.read_text(encoding="utf-8", errors="ignore"))
        for key in ["electron-saved-workspace-roots", "project-order", "active-workspace-roots"]:
            registry[key] = [p for p in gs.get(key, []) if path_matches_project(p) or win_norm(p) in project_roots]
        registry["projectless-thread-ids"] = [tid for tid in gs.get("projectless-thread-ids", []) if tid in thread_ids]
        hints = gs.get("thread-workspace-root-hints", {})
        registry["thread-workspace-root-hints"] = {tid: path for tid, path in hints.items() if tid in thread_ids or path_matches_project(path)}
        outputs = gs.get("thread-projectless-output-directories", {})
        registry["thread-projectless-output-directories"] = {tid: path for tid, path in outputs.items() if tid in thread_ids or path_matches_project(path)}
        perms = (gs.get("electron-persisted-atom-state", {}) or {}).get("heartbeat-thread-permissions-by-id", {})
        registry["heartbeat-thread-permissions-by-id"] = {tid: perms[tid] for tid in thread_ids if tid in perms}
    except Exception:
        pass

(metadata_dir / "path_map.json").write_text(json.dumps({
    "schema": 3,
    "source_os": "Windows",
    "target_os": "Mac",
    "projects": project_entries
}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

(metadata_dir / "selected_chats.json").write_text(json.dumps({
    "schema": 3,
    "selected_chats": selected_chat_files
}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

(metadata_dir / "thread_index_export.json").write_text(json.dumps({
    "schema": 3,
    "source_os": "Windows",
    "source_state_files": state_files,
    "selected_thread_ids": selected_ids,
    "threads": thread_rows
}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

(metadata_dir / "project_ui_registry_export.json").write_text(json.dumps({
    "schema": 3,
    "source_os": "Windows",
    "source_global_state": str(global_state_path) if global_state_path else "",
    "project_registry": registry
}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
'@
    $tmpPy = Join-Path $Metadata "export_ui_ready_metadata.py"
    Write-RawUtf8NoBomLf -Path $tmpPy -Text $pyCode
    & $python.command @($python.args) $tmpPy $requestPath
    if ($LASTEXITCODE -ne 0) {
        throw "UI-ready metadata export failed."
    }
    Remove-Item -LiteralPath $tmpPy -Force -ErrorAction SilentlyContinue
}

function New-CrossPlatformZip {
    param(
        [Parameter(Mandatory=$true)][string]$SourceDirectory,
        [Parameter(Mandatory=$true)][string]$DestinationZip
    )

    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem

    if (Test-Path -LiteralPath $DestinationZip) {
        Remove-Item -LiteralPath $DestinationZip -Force
    }

    $sourceFull = (Resolve-Path -LiteralPath $SourceDirectory).Path
    $parent = Split-Path -Parent $sourceFull
    $safeDate = [DateTimeOffset](Get-Date "2020-01-01T00:00:00Z")
    $archive = [System.IO.Compression.ZipFile]::Open($DestinationZip, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        Get-ChildItem -LiteralPath $sourceFull -Recurse -Force -Directory | ForEach-Object {
            $entryName = $_.FullName.Substring($parent.Length + 1).Replace("\", "/") + "/"
            $entry = $archive.CreateEntry($entryName)
            $entry.LastWriteTime = $safeDate
        }

        Get-ChildItem -LiteralPath $sourceFull -Recurse -Force -File | ForEach-Object {
            $entryName = $_.FullName.Substring($parent.Length + 1).Replace("\", "/")
            $entry = $archive.CreateEntry($entryName, [System.IO.Compression.CompressionLevel]::Optimal)
            $entry.LastWriteTime = $safeDate
            $inputStream = [System.IO.File]::OpenRead($_.FullName)
            try {
                $outputStream = $entry.Open()
                try {
                    $inputStream.CopyTo($outputStream)
                } finally {
                    $outputStream.Dispose()
                }
            } finally {
                $inputStream.Dispose()
            }
        }
    } finally {
        $archive.Dispose()
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

foreach ($chatPath in $SelectedChat) {
    if (Test-Path -LiteralPath $chatPath -PathType Leaf) {
        Copy-Item -LiteralPath $chatPath -Destination (Join-Path $Stage "selected_chats\$(Split-Path -Leaf $chatPath)") -Force
        Ensure-SelectedChatInSessions -ChatPath $chatPath
    } else {
        Write-Warning "Missing selected chat: $chatPath"
    }
}

Ensure-SessionIndex
Export-UiReadyMetadata

$SensitiveReport = Join-Path $Docs "SENSITIVE-FILES.txt"
$sensitivePaths = @(
    (Join-Path $env:USERPROFILE ".codex\auth.json"),
    (Join-Path $env:APPDATA "Codex\Cookies"),
    (Join-Path $env:APPDATA "Codex\Default\Login Data"),
    (Join-Path $env:APPDATA "Codex\Local Storage"),
    (Join-Path $env:APPDATA "Codex\Session Storage")
) | Where-Object { Test-Path -LiteralPath $_ }

Write-Utf8NoBomLf -Path $SensitiveReport -Lines @(
    "Sensitive files report"
    "Generated: $Stamp"
    "Mode: $Mode"
    ""
    "The following paths exist or matched common sensitive patterns on the source Windows machine."
    "Contents are intentionally not printed."
    ""
    $sensitivePaths
)

if ($Mode -ne "standard") {
    $envReport = Join-Path $Docs "ENV-INVENTORY.txt"
    $toolLines = foreach ($cmd in @("codex", "git", "node", "npm", "pnpm", "yarn", "python", "py", "uv", "cargo", "rustc", "go")) {
        $found = Get-Command $cmd -ErrorAction SilentlyContinue
        if ($found) {
            try { "${cmd}: $(& $cmd --version 2>$null | Select-Object -First 1)" } catch { "${cmd}: $($found.Source)" }
        }
    }
    Write-Utf8NoBomLf -Path $envReport -Lines @(
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
    )
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
   bash Restore-Codex-To-Mac.sh --restore-projects
3. Verify:
   bash Verify-Codex-Mac-Restore.sh --json

Project folders, if included, are under projects/. By default, the Mac restore script copies them to ~/Documents/Codex-Restored-Projects when --restore-projects is passed.

For Mac restores with projects, the restore script invokes:
  /Applications/Codex.app/Contents/Resources/codex app <restored-project-path>

This official Codex Desktop entry point registers/opens restored projects in the app-visible project list. Hand-editing .codex-global-state.json alone is not enough, because a running Codex Desktop process can overwrite that file on quit.

Selected chat files, if included, are under selected_chats/. They are duplicated there for inspection and should also appear in home/.codex/sessions when restored.

Restore scripts merge into the target Codex home by default. Existing target login/config identity files are preserved. Use --replace-codex-home / -ReplaceCodexHome only when you intentionally want a destructive full replacement. State databases are not overwritten unless --replace-state / -ReplaceState is passed.

Schema v3 packages include metadata/thread_index_export.json, metadata/path_map.json, and metadata/project_ui_registry_export.json for UI-ready Mac restore checks, plus a project registration report after Mac restore.

By default this package excludes browser login state, auth.json, .env files, and private keys. If it was created with full-with-secrets, treat it like a password vault.
"@
Write-RawUtf8NoBomLf -Path (Join-Path $Stage "README-Restore.txt") -Text $readme

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

$codexHome = Join-Path $Stage "home\.codex"
$projectsRoot = Join-Path $Stage "projects"
$counts = [ordered]@{
    sessions = Count-Files -Path (Join-Path $codexHome "sessions") -Filter "*.jsonl"
    archived_sessions = Count-Files -Path (Join-Path $codexHome "archived_sessions") -Filter "*.jsonl"
    skills = Count-Files -Path (Join-Path $codexHome "skills") -Filter "SKILL.md"
    plugin_manifests = Count-Files -Path (Join-Path $codexHome "plugins\cache") -Filter "plugin.json"
    generated_images = Count-Files -Path (Join-Path $codexHome "generated_images")
    sqlite_files = Count-Files -Path $codexHome -Filter "*.sqlite"
    session_index_entries = Count-JsonlEntries -Path (Join-Path $codexHome "session_index.jsonl")
    projects = Count-ImmediateDirectories -Path $projectsRoot
    selected_chats = Count-Files -Path (Join-Path $Stage "selected_chats") -Filter "*.jsonl"
    thread_index_export = Count-Files -Path $Metadata -Filter "thread_index_export.json"
    path_map = Count-Files -Path $Metadata -Filter "path_map.json"
    project_ui_registry_export = Count-Files -Path $Metadata -Filter "project_ui_registry_export.json"
}

$manifestText = @()
$manifestText += "created_at=$Stamp"
$manifestText += "source_os=Windows"
$manifestText += "package_schema_version=$PackageSchemaVersion"
$manifestText += "source_home=$env:USERPROFILE"
$manifestText += "mode=$Mode"
$manifestText += "package=$ZipPath"
$manifestText += "projects=$($Project -join ' ')"
$manifestText += "selected_chats=$($SelectedChat -join ' ')"
$manifestText += ""
$manifestText += "[counts]"
foreach ($key in $counts.Keys) {
    $manifestText += "$key=$($counts[$key])"
}
$manifestText += ""
$manifestText += "[exclude_strategy]"
$manifestText += $ExcludeSummary
Write-Utf8NoBomLf -Path (Join-Path $Stage "MANIFEST.txt") -Lines $manifestText

$manifestJson = [ordered]@{
    created_at = $Stamp
    source_os = "Windows"
    package_schema_version = $PackageSchemaVersion
    source_home = $env:USERPROFILE
    mode = $Mode
    package = $ZipPath
    projects = $Project
    selected_chats = $SelectedChat
    counts = $counts
    exclude_strategy = $ExcludeSummary
    notes = @(
        "Path mappings are recorded rather than applied to JSONL sessions in place.",
        "Schema v3 metadata exports thread rows, path mapping, and non-sensitive project UI registry hints for target restore.",
        "On Mac, project restore invokes /Applications/Codex.app/Contents/Resources/codex app <restored-project-path> so Codex Desktop registers restored projects.",
        "Use docs/SENSITIVE-FILES.txt to review suspected sensitive files without exposing values.",
        "Run the restore script for the target OS only after closing Codex on that target."
    )
}
Write-RawUtf8NoBomLf -Path (Join-Path $Stage "MANIFEST.json") -Text ($manifestJson | ConvertTo-Json -Depth 8)

$shaFile = Join-Path $Stage "SHA256SUMS.txt"
$shaLines = Get-ChildItem -LiteralPath $Stage -Recurse -Force -File |
    Where-Object { $_.FullName -ne $shaFile } |
    ForEach-Object {
        $rel = $_.FullName.Substring($Stage.Length + 1).Replace("\", "/")
        "$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLower())  $rel"
    }
Write-Utf8NoBomLf -Path $shaFile -Lines $shaLines

New-CrossPlatformZip -SourceDirectory $Stage -DestinationZip $ZipPath

Write-Host "Created: $ZipPath"
Write-Host "Stage: $Stage"
