$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$builder = Join-Path $repoRoot 'scripts\build_redskill_package.ps1'
$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ('codex-rehome-redskill-test-' + [guid]::NewGuid().ToString('N'))

try {
    if (-not (Test-Path -LiteralPath $builder -PathType Leaf)) {
        throw "Missing Red Skill builder: $builder"
    }

    & $builder -Out $tempRoot

    $skillRoot = Join-Path $tempRoot 'codex-rehome'
    $zipPath = Join-Path $tempRoot 'codex-rehome-redskill.zip'
    $uploadMetadataPath = Join-Path $tempRoot 'redskill-upload.json'
    $skillFile = Join-Path $skillRoot 'SKILL.md'

    foreach ($required in @(
        $skillFile,
        (Join-Path $skillRoot 'scripts\create_mac_codex_migration_package.sh'),
        (Join-Path $skillRoot 'scripts\create_windows_codex_migration_package.ps1'),
        (Join-Path $skillRoot 'scripts\restore_codex_to_mac.sh'),
        (Join-Path $skillRoot 'scripts\restore_codex_to_windows.ps1'),
        (Join-Path $skillRoot 'scripts\verify_mac_codex_restore.sh'),
        (Join-Path $skillRoot 'scripts\verify_windows_codex_restore.ps1'),
        (Join-Path $skillRoot 'references\path-map.md'),
        $zipPath,
        $uploadMetadataPath
    )) {
        if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
            throw "Required Red Skill package file missing: $required"
        }
    }

    $uploadMetadata = Get-Content -LiteralPath $uploadMetadataPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $expectedChineseName = [System.Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('Q29kZXgg5pCs5a62'))
    if ($uploadMetadata.name -ne 'Codex Rehome') { throw 'Upload metadata name is incorrect' }
    if ($uploadMetadata.title -ne $expectedChineseName) { throw 'Upload metadata title must match the Chinese public name' }
    if ($uploadMetadata.source_type -ne 'original') { throw 'Upload metadata must mark the Skill as original' }
    if ($uploadMetadata.repository -ne 'https://github.com/CalebYcj/codex-rehome') { throw 'Upload metadata repository is incorrect' }
    if ([string]::IsNullOrWhiteSpace($uploadMetadata.title) -or [string]::IsNullOrWhiteSpace($uploadMetadata.description)) {
        throw 'Upload metadata title and description are required'
    }

    $agentMetadataPath = Join-Path $skillRoot 'agents\openai.yaml'
    if (-not (Test-Path -LiteralPath $agentMetadataPath -PathType Leaf)) {
        throw 'Red Skill package is missing agents/openai.yaml'
    }
    $agentMetadata = Get-Content -LiteralPath $agentMetadataPath -Raw -Encoding UTF8
    if (-not $agentMetadata.Contains(('display_name: "' + $expectedChineseName + '"'))) {
        throw 'Agent display name must match the Chinese public name'
    }

    $skillText = Get-Content -LiteralPath $skillFile -Raw -Encoding UTF8
    $utf8 = [System.Text.Encoding]::UTF8
    $requiredUtf8Phrases = @(
        '5YWI5Yik5pat55So5oi3546w5Zyo5aSE5LqO5ZOq5Liq6Zi25q61',
        '5Y6f55S16ISR',
        '5paw55S16ISR',
        '6YeN6KOF57O757uf',
        '6buY6K6k5L2/55SoIG1lcmdlIHJlc3RvcmU=',
        '6aG555uu5paH5Lu25aS55LiN5Lya5Zug5Li65aSN5Yi2IENvZGV4IOaVsOaNruiAjOiHquWKqOWMheWQqw=='
    ) | ForEach-Object { $utf8.GetString([Convert]::FromBase64String($_)) }
    foreach ($phrase in @(
        $requiredUtf8Phrases
        'Mac -> Windows',
        'Windows -> Mac',
        'Windows -> Windows',
        'Mac -> Mac',
        'codex app'
    )) {
        if (-not $skillText.Contains($phrase)) {
            throw "Red Skill instructions are missing required phrase: $phrase"
        }
    }

    $forbiddenNames = @('auth.json', '.env', 'Cookies', 'Login Data', 'Local Storage', 'Session Storage', '.git', 'node_modules', '.venv', 'venv')
    $packagedPaths = Get-ChildItem -LiteralPath $skillRoot -Recurse -Force | ForEach-Object { $_.FullName.Substring($skillRoot.Length + 1) }
    foreach ($forbidden in $forbiddenNames) {
        if ($packagedPaths | Where-Object { ($_ -split '[\\/]') -contains $forbidden }) {
            throw "Forbidden path found in Red Skill package: $forbidden"
        }
    }

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [System.IO.Compression.ZipFile]::OpenRead($zipPath)
    try {
        $entries = @($archive.Entries | ForEach-Object { $_.FullName })
        if ($entries -notcontains 'codex-rehome/SKILL.md') {
            throw 'ZIP is missing codex-rehome/SKILL.md'
        }
        if ($entries | Where-Object { $_ -match '\\' }) {
            throw 'ZIP contains Windows backslash entry paths'
        }
    }
    finally {
        $archive.Dispose()
    }

    Write-Output "PASS redskill_package files=$($packagedPaths.Count) zip=$zipPath"
}
finally {
    if (Test-Path -LiteralPath $tempRoot) {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force
    }
}
