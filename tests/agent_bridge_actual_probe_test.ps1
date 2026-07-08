$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
$InspectScript = Join-Path $RepoRoot 'codex-rehome\scripts\inspect_claude_agent_sources.py'

if (-not (Test-Path -LiteralPath $InspectScript)) {
    throw "Missing inspect script: $InspectScript"
}

$json = python $InspectScript --json
$report = $json | ConvertFrom-Json

if ($report.schema_version -ne 1) {
    throw "Unexpected schema version: $($report.schema_version)"
}

if (-not $report.status) {
    throw "Missing status"
}

if ($null -eq $report.sources -or $report.sources.Count -lt 1) {
    throw "Expected at least one inspected source"
}

Write-Output "Agent Bridge inspect status: $($report.status)"
Write-Output "Exportable Claude JSONL count: $($report.exportable_jsonl_count)"
if ($report.logs.has_pro_or_max_required_error) {
    Write-Output "Detected Claude Code Pro/Max entitlement requirement in local logs."
}
