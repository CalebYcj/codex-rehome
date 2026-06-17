$ErrorActionPreference = "Stop"

$Paths = @(
    (Join-Path $env:USERPROFILE ".codex"),
    (Join-Path $env:APPDATA "Codex"),
    (Join-Path $env:APPDATA "com.openai.codex"),
    (Join-Path $env:APPDATA "OpenAI\Codex"),
    (Join-Path $env:LOCALAPPDATA "Codex"),
    (Join-Path $env:LOCALAPPDATA "com.openai.codex"),
    (Join-Path $env:LOCALAPPDATA "com.openai.sky.CUAService"),
    (Join-Path $env:LOCALAPPDATA "com.openai.sky.CUAService.cli")
)

function Get-DirectorySize {
    param([string]$Path)

    if (-not (Test-Path -LiteralPath $Path)) {
        return $null
    }

    $Bytes = (Get-ChildItem -LiteralPath $Path -Recurse -Force -ErrorAction SilentlyContinue |
        Measure-Object -Property Length -Sum).Sum
    if ($null -eq $Bytes) { $Bytes = 0 }
    return [Math]::Round($Bytes / 1MB, 2)
}

Write-Host "Codex inventory for $env:COMPUTERNAME"
Write-Host "User: $env:USERNAME"
Write-Host ""

foreach ($Path in $Paths) {
    $Size = Get-DirectorySize -Path $Path
    if ($null -eq $Size) {
        Write-Host "[missing] $Path"
    } else {
        Write-Host "[found]   $Path ($Size MB)"
    }
}

Write-Host ""
Write-Host "Likely project folders:"
Get-ChildItem -LiteralPath (Join-Path $env:USERPROFILE "Documents") -Directory -ErrorAction SilentlyContinue |
    Where-Object {
        (Test-Path -LiteralPath (Join-Path $_.FullName ".git") -PathType Container) -or
        (Test-Path -LiteralPath (Join-Path $_.FullName ".agents") -PathType Container)
    } |
    Select-Object -First 30 -ExpandProperty FullName |
    ForEach-Object { Write-Host "  $_" }
