# Generates src/United.App/Assets/united.ico from scratch (no external assets).
# Style: dark rounded square + white "U" — matches the tray glyph drawn at runtime.
param(
    [string]$Out = "$PSScriptRoot\..\src\United.App\Assets\united.ico"
)

Add-Type -AssemblyName System.Drawing

function New-IconBitmap([int]$size) {
    $bmp = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit

    $pad = [Math]::Max(1, $size * 0.03)
    $radius = [Math]::Max(2, $size * 0.22)
    $rect = New-Object System.Drawing.RectangleF($pad, $pad, ($size - 2 * $pad), ($size - 2 * $pad))

    # Rounded square background
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $radius * 2
    $path.AddArc($rect.X, $rect.Y, $d, $d, 180, 90)
    $path.AddArc($rect.Right - $d, $rect.Y, $d, $d, 270, 90)
    $path.AddArc($rect.Right - $d, $rect.Bottom - $d, $d, $d, 0, 90)
    $path.AddArc($rect.X, $rect.Bottom - $d, $d, $d, 90, 90)
    $path.CloseFigure()
    $brush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(31, 31, 31))
    $g.FillPath($brush, $path)

    # White "U"
    $fontSize = [Math]::Max(6, $size * 0.56)
    $font = New-Object System.Drawing.Font('Segoe UI', $fontSize, [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
    $white = [System.Drawing.Brushes]::White
    $fmt = New-Object System.Drawing.StringFormat
    $fmt.Alignment = [System.Drawing.StringAlignment]::Center
    $fmt.LineAlignment = [System.Drawing.StringAlignment]::Center
    $g.DrawString('U', $font, $white, (New-Object System.Drawing.RectangleF(0, ($size * -0.02), $size, $size)), $fmt)

    # Accent underline (subtle brand accent, matches UI accent color)
    $accent = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(0, 103, 192))
    $uw = $size * 0.34
    $uh = [Math]::Max(1, $size * 0.06)
    $g.FillRectangle($accent, (($size - $uw) / 2), ($size * 0.78), $uw, $uh)

    $g.Dispose(); $font.Dispose(); $brush.Dispose(); $accent.Dispose(); $path.Dispose(); $fmt.Dispose()
    return $bmp
}

function Convert-ToIco([System.Drawing.Bitmap[]]$bitmaps, [string]$path) {
    $ms = New-Object System.IO.MemoryStream
    $bw = New-Object System.IO.BinaryWriter($ms)
    $bw.Write([uint16]0)            # reserved
    $bw.Write([uint16]1)            # type: icon
    $bw.Write([uint16]$bitmaps.Count)

    $entries = @()
    foreach ($bmp in $bitmaps) {
        $png = New-Object System.IO.MemoryStream
        $bmp.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
        $entries += ,@($png.ToArray(), $bmp.Width, $bmp.Height)
        $bmp.Dispose()
    }

    $offset = 6 + 16 * $entries.Count
    foreach ($e in $entries) {
        $data = $e[0]; $w = $e[1]; $h = $e[2]
        $bw.Write([byte]($(if ($w -ge 256) { 0 } else { $w })))
        $bw.Write([byte]($(if ($h -ge 256) { 0 } else { $h })))
        $bw.Write([byte]0)          # palette colors
        $bw.Write([byte]0)          # reserved
        $bw.Write([uint16]1)        # color planes
        $bw.Write([uint16]32)       # bpp
        $bw.Write([uint32]$data.Length)
        $bw.Write([uint32]$offset)
        $offset += $data.Length
    }
    foreach ($e in $entries) { $bw.Write($e[0]) }
    $bw.Flush()
    [IO.File]::WriteAllBytes($path, $ms.ToArray())
    $bw.Dispose(); $ms.Dispose()
}

$outDir = Split-Path $Out -Parent
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$bitmaps = @()
foreach ($size in 16, 24, 32, 48, 64, 128, 256) { $bitmaps += New-IconBitmap $size }
Convert-ToIco $bitmaps $Out
Write-Output "Icon written: $((Resolve-Path $Out).Path) ($((Get-Item $Out).Length) bytes)"
