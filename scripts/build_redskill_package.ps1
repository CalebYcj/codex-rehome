param(
    [string]$Out
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($Out)) {
    $Out = Join-Path $repoRoot 'dist\redskill'
}

$sourceSkill = Join-Path $repoRoot 'codex-rehome'
$redSkillSource = Join-Path $repoRoot 'redskill'
$skillRoot = Join-Path $Out 'codex-rehome'
$zipPath = Join-Path $Out 'codex-rehome-redskill.zip'
$metadataOut = Join-Path $Out 'redskill-upload.json'

foreach ($required in @(
    (Join-Path $redSkillSource 'SKILL.md'),
    (Join-Path $redSkillSource 'upload-metadata.json'),
    (Join-Path $sourceSkill 'scripts'),
    (Join-Path $sourceSkill 'references')
)) {
    if (-not (Test-Path -LiteralPath $required)) {
        throw "Required source missing: $required"
    }
}

New-Item -ItemType Directory -Path $Out -Force | Out-Null
if (Test-Path -LiteralPath $skillRoot) {
    Remove-Item -LiteralPath $skillRoot -Recurse -Force
}
if (Test-Path -LiteralPath $zipPath) {
    Remove-Item -LiteralPath $zipPath -Force
}

New-Item -ItemType Directory -Path $skillRoot -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $redSkillSource 'SKILL.md') -Destination (Join-Path $skillRoot 'SKILL.md')
Copy-Item -LiteralPath (Join-Path $sourceSkill 'scripts') -Destination (Join-Path $skillRoot 'scripts') -Recurse
Copy-Item -LiteralPath (Join-Path $sourceSkill 'references') -Destination (Join-Path $skillRoot 'references') -Recurse

$redAgents = Join-Path $redSkillSource 'agents'
if (Test-Path -LiteralPath $redAgents -PathType Container) {
    Copy-Item -LiteralPath $redAgents -Destination (Join-Path $skillRoot 'agents') -Recurse
}

Copy-Item -LiteralPath (Join-Path $redSkillSource 'upload-metadata.json') -Destination $metadataOut -Force

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::Open($zipPath, [System.IO.Compression.ZipArchiveMode]::Create)
try {
    $base = Split-Path -Parent $skillRoot
    Get-ChildItem -LiteralPath $skillRoot -Recurse -File | ForEach-Object {
        $entryName = $_.FullName.Substring($base.Length + 1).Replace('\', '/')
        [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
            $archive,
            $_.FullName,
            $entryName,
            [System.IO.Compression.CompressionLevel]::Optimal
        ) | Out-Null
    }
}
finally {
    $archive.Dispose()
}

Write-Output "Red Skill folder: $skillRoot"
Write-Output "Red Skill zip: $zipPath"
Write-Output "Upload metadata: $metadataOut"
