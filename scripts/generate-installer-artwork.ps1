# Regenerate the Windows NSIS artwork with built-in drawing APIs; no design-tool
# dependency is needed for builds. Checked-in 24-bit BMPs are used by all runners.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$artworkRoot = Join-Path $PSScriptRoot '../desktop/src-tauri/installer'
New-Item -ItemType Directory -Force -Path $artworkRoot | Out-Null
$ink = [Drawing.ColorTranslator]::FromHtml('#1d1b17')
$paper = [Drawing.ColorTranslator]::FromHtml('#fbf8f1')
$muted = [Drawing.ColorTranslator]::FromHtml('#68645d')
$markImage = [Drawing.Image]::FromFile((Join-Path $PSScriptRoot '../desktop/brand-mark.png'))

function Draw-Mark($graphics, [single]$x, [single]$y, [single]$size) {
    # Rendered from the same brand-mark.svg used by the app and platform icons.
    $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $graphics.DrawImage($markImage, $x, $y, $size, $size)
}

function Draw-Label($graphics, [string]$text, [single]$size, [single]$x, [single]$y, $color, [bool]$bold = $false) {
    $style = if ($bold) { [Drawing.FontStyle]::Bold } else { [Drawing.FontStyle]::Regular }
    $font = [Drawing.Font]::new('Segoe UI', $size, $style, [Drawing.GraphicsUnit]::Pixel)
    $brush = [Drawing.SolidBrush]::new($color)
    try { $graphics.DrawString($text, $font, $brush, $x, $y) }
    finally { $font.Dispose(); $brush.Dispose() }
}

function Save-Artwork([string]$name, [int]$width, [int]$height, $background, [scriptblock]$draw) {
    $canvas = [Drawing.Bitmap]::new($width * 4, $height * 4)
    $graphics = [Drawing.Graphics]::FromImage($canvas)
    $bitmap = [Drawing.Bitmap]::new($width, $height, [Drawing.Imaging.PixelFormat]::Format24bppRgb)
    $resizer = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.Clear($background)
        $graphics.ScaleTransform(4, 4)
        $graphics.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $graphics.TextRenderingHint = [Drawing.Text.TextRenderingHint]::AntiAliasGridFit
        & $draw $graphics
        $resizer.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $resizer.DrawImage($canvas, 0, 0, $width, $height)
        $bitmap.Save((Join-Path $artworkRoot "$name.bmp"), [Drawing.Imaging.ImageFormat]::Bmp)
        $bitmap.Save((Join-Path $artworkRoot "$name.png"), [Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $graphics.Dispose(); $resizer.Dispose(); $canvas.Dispose(); $bitmap.Dispose()
    }
}

Save-Artwork 'sidebar' 164 314 $paper {
    param($graphics)
    Draw-Label $graphics 'DESKTOP' 9 22 24 $muted
    Draw-Mark $graphics 20 47 124
    Draw-Label $graphics 'ReHome' 24 20 175 $ink $true
    Draw-Label $graphics "Your work.`nA new home." 12 22 216 $muted
    Draw-Label $graphics 'OFFLINE MIGRATION' 8 22 282 $muted
}
Save-Artwork 'header' 150 57 ([Drawing.Color]::White) {
    param($graphics)
    Draw-Mark $graphics 0 5 47
    Draw-Label $graphics 'ReHome' 18 48 17 $ink $true
}
Write-Output 'Generated NSIS 24-bit sidebar 164x314 and header 150x57.'
$markImage.Dispose()
