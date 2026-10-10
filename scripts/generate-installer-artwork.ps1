# Regenerate the Windows NSIS artwork with built-in drawing APIs; no design-tool
# dependency is needed for builds. Checked-in 24-bit BMPs are used by all runners.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$artworkRoot = Join-Path $PSScriptRoot '../desktop/src-tauri/installer'
New-Item -ItemType Directory -Force -Path $artworkRoot | Out-Null
$ink = [Drawing.ColorTranslator]::FromHtml('#2e4c60')
$paper = [Drawing.ColorTranslator]::FromHtml('#f7f8f8')
$muted = [Drawing.ColorTranslator]::FromHtml('#c8d4dc')

function Draw-Mark($graphics, [single]$x, [single]$y, [single]$size, $color) {
    # Same two interlocking doorways as desktop/app-icon.svg.
    $saved = $graphics.Save()
    $graphics.TranslateTransform($x, $y)
    $graphics.ScaleTransform($size / 512, $size / 512)
    $pen = [Drawing.Pen]::new($color, 27)
    $pen.StartCap = $pen.EndCap = [Drawing.Drawing2D.LineCap]::Round
    $pen.LineJoin = [Drawing.Drawing2D.LineJoin]::Round
    try {
        $graphics.DrawLines($pen, [Drawing.PointF[]]@(
            [Drawing.PointF]::new(214,367), [Drawing.PointF]::new(113,367),
            [Drawing.PointF]::new(113,126), [Drawing.PointF]::new(302,126),
            [Drawing.PointF]::new(302,227)))
        $graphics.DrawLines($pen, [Drawing.PointF[]]@(
            [Drawing.PointF]::new(290,164), [Drawing.PointF]::new(391,164),
            [Drawing.PointF]::new(391,404), [Drawing.PointF]::new(202,404),
            [Drawing.PointF]::new(202,303)))
        $graphics.DrawLine($pen, 202,265,315,265)
    } finally { $pen.Dispose(); $graphics.Restore($saved) }
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

Save-Artwork 'sidebar' 164 314 $ink {
    param($graphics)
    Draw-Label $graphics 'DESKTOP' 9 22 24 $muted
    Draw-Mark $graphics 14 51 136 $paper
    Draw-Label $graphics 'ReHome' 24 20 175 $paper $true
    Draw-Label $graphics "Your work.`nA new home." 12 22 216 $muted
    Draw-Label $graphics 'OFFLINE MIGRATION' 8 22 282 $muted
}
Save-Artwork 'header' 150 57 ([Drawing.Color]::White) {
    param($graphics)
    Draw-Mark $graphics 0 5 47 $ink
    Draw-Label $graphics 'ReHome' 18 48 17 $ink $true
}
Write-Output 'Generated NSIS 24-bit sidebar 164x314 and header 150x57.'
