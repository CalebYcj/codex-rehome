# Read embedded icons without launching or installing either executable.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$root = Split-Path -Parent $PSScriptRoot
$referencePath = Join-Path $root 'desktop/src-tauri/icons/icon.ico'
$application = Join-Path $root 'desktop/src-tauri/target/release/rehome-desktop.exe'
$installers = @(Get-ChildItem (Join-Path $root 'desktop/src-tauri/target/release/bundle/nsis') -Filter '*-setup.exe')
if ($installers.Count -ne 1) { throw 'Expected exactly one Windows installer' }
foreach ($executable in @($application, $installers[0].FullName)) {
    $icon = [Drawing.Icon]::ExtractAssociatedIcon($executable)
    if (!$icon) { throw "No embedded icon: $executable" }
    $bitmap = $icon.ToBitmap()
    $referenceIcon = [Drawing.Icon]::new($referencePath, $bitmap.Width, $bitmap.Height)
    $reference = $referenceIcon.ToBitmap()
    try {
        for ($y=0; $y -lt $bitmap.Height; $y++) {
            for ($x=0; $x -lt $bitmap.Width; $x++) {
                if ($bitmap.GetPixel($x,$y).ToArgb() -ne $reference.GetPixel($x,$y).ToArgb()) {
                    throw "Executable branding differs from the current logo: $executable"
                }
            }
        }
        Write-Output "PASS embedded application/setup icon: $([IO.Path]::GetFileName($executable))"
    } finally { $bitmap.Dispose(); $reference.Dispose(); $icon.Dispose(); $referenceIcon.Dispose() }
}
