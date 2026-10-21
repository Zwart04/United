# Converts official brand SVG marks (simple-icons, CC0) into a WPF ResourceDictionary
# of colored DrawingImage resources — pure XAML vectors, no libraries, no raster.
#
# Two composition styles:
#   markOnWhite : the SVG path already contains the badge shape (circle + counter holes).
#                 Draw a white circle under it so the holes read white — official look.
#   badge       : draw a brand-colored tile (circle/rounded rect/gradient), then the
#                 glyph centered on top in white, scaled by <scale> with offset <tx>.
#
# Output: src/United.App/Resources/BrandIcons.xaml   (keys: Brand.{PascalCase})
param(
    [string]$LogosDir = "$PSScriptRoot\logos",
    [string]$Out = "$PSScriptRoot\..\src\United.App\Resources\BrandIcons.xaml"
)

$spec = @(
    # --- official full-mark paths (badge shape is inside the path) ---
    @{ Key = 'Whatsapp';  Slug = 'whatsapp';  Style = 'wa' }
    @{ Key = 'Telegram';  Slug = 'telegram';  Style = 'markOnWhite'; Brush = '<LinearGradientBrush StartPoint="0,0" EndPoint="0,1">
        <GradientStop Color="#37BBFE" Offset="0" />
        <GradientStop Color="#1E96C8" Offset="1" />
      </LinearGradientBrush>' }
    @{ Key = 'Messenger'; Slug = 'messenger'; Style = 'markOnWhite'; Brush = '<LinearGradientBrush StartPoint="0,0" EndPoint="1,1">
        <GradientStop Color="#00B2FF" Offset="0" />
        <GradientStop Color="#006AFF" Offset="0.45" />
        <GradientStop Color="#A033FF" Offset="0.75" />
        <GradientStop Color="#FF5280" Offset="1" />
      </LinearGradientBrush>' }
    @{ Key = 'Reddit';    Slug = 'reddit';    Style = 'markOnWhite'; Brush = '#FF4500' }
    @{ Key = 'Youtube';   Slug = 'youtube';   Style = 'markOnWhite'; Brush = '#FF0000' }
    @{ Key = 'Facebook';  Slug = 'facebook';  Style = 'markOnWhite'; Brush = '#0866FF' }

    # --- glyph on brand tile ---
    @{ Key = 'Instagram'; Slug = 'instagram'; Style = 'badge'; Shape = 'rect'; Scale = '0.68'; Tx = '3.84'
       Brush = '<LinearGradientBrush StartPoint="0,1" EndPoint="1,0">
        <GradientStop Color="#FEDA75" Offset="0" />
        <GradientStop Color="#FA7E1E" Offset="0.25" />
        <GradientStop Color="#D62976" Offset="0.55" />
        <GradientStop Color="#962FBF" Offset="0.8" />
        <GradientStop Color="#4F5BD5" Offset="1" />
      </LinearGradientBrush>' }
    @{ Key = 'Discord';   Slug = 'discord';   Style = 'badge'; Shape = 'rect'; Scale = '0.78'; Tx = '2.64'; Brush = '#5865F2' }
    @{ Key = 'Threads';   Slug = 'threads';   Style = 'badge'; Shape = 'rect'; Scale = '0.68'; Tx = '3.84'; Brush = '#000000' }
    @{ Key = 'X';         Slug = 'x';         Style = 'badge'; Shape = 'rect'; Scale = '0.72'; Tx = '3.36'; Brush = '#000000' }
    @{ Key = 'Linkedin';  Slug = 'linkedin';  Style = 'badge'; Shape = 'rect'; Scale = '0.70'; Tx = '3.60'; Brush = '#0A66C2' }
    @{ Key = 'Bluesky';   Slug = 'bluesky';   Style = 'badge'; Shape = 'rect'; Scale = '0.72'; Tx = '3.36'; Brush = '#0085FF' }
)

function Get-PathData([string]$slug) {
    $svg = Get-Content -Path (Join-Path $LogosDir "$slug.svg") -Raw
    if ($svg -notmatch '<path[^>]*\sd="([^"]+)"') { throw "No path data in $slug.svg" }
    return $Matches[1]
}

# Extracts the `d` attribute of a <path> matching a fill expression (optionally evenodd).
function Get-PathByFill([string]$svgPath, [string]$fillPattern, [switch]$EvenOdd) {
    $raw = Get-Content -Path $svgPath -Raw
    foreach ($m in [regex]::Matches($raw, '<path\b[^>]*>')) {
        $tag = $m.Value
        if ($tag -notmatch $fillPattern) { continue }
        if ($EvenOdd -and $tag -notmatch 'fill-rule="evenodd"') { continue }
        if ($tag -match '\bd="([^"]+)"') { return $Matches[1] }
    }
    throw "No path matching $fillPattern in $svgPath"
}

function Emit-Drawing([string]$key, [string]$body) {
    return @"
    <DrawingImage x:Key="Brand.$key">
      <DrawingImage.Drawing>
        <DrawingGroup>
$body
        </DrawingGroup>
      </DrawingImage.Drawing>
    </DrawingImage>
"@
}

$blocks = foreach ($b in $spec) {
    $d = Get-PathData $b.Slug
    $glyph = $d -replace '&', '&amp;'   # defensive; path data has no entities

    if ($b.Style -eq 'wa') {
        # Authentic WhatsApp logo (Wikimedia official SVG): gradient green bubble
        # with tail + white handset, used verbatim and scaled from its 175.552 viewBox.
        $official = Join-Path $LogosDir 'whatsapp-official.svg'
        $bubble = Get-PathByFill $official 'url\(#b\)'
        $handset = Get-PathByFill $official '#fff' -EvenOdd
        $body = @"
          <DrawingGroup>
            <DrawingGroup.Transform>
              <MatrixTransform Matrix="0.136711,0,0,0.136711,0.023,0" />
            </DrawingGroup.Transform>
            <GeometryDrawing>
              <GeometryDrawing.Brush>
                <LinearGradientBrush StartPoint="0,0" EndPoint="0,1">
                  <GradientStop Color="#57D163" Offset="0" />
                  <GradientStop Color="#23B33A" Offset="1" />
                </LinearGradientBrush>
              </GeometryDrawing.Brush>
              <GeometryDrawing.Geometry>
                <PathGeometry Figures="$bubble" />
              </GeometryDrawing.Geometry>
            </GeometryDrawing>
            <GeometryDrawing Brush="White">
              <GeometryDrawing.Geometry>
                <PathGeometry Figures="$handset" FillRule="EvenOdd" />
              </GeometryDrawing.Geometry>
            </GeometryDrawing>
          </DrawingGroup>
"@
    }
    elseif ($b.Style -eq 'markOnWhite') {
        $underlay = @"
          <GeometryDrawing Brush="White">
            <GeometryDrawing.Geometry>
              <EllipseGeometry Center="12,12" RadiusX="12" RadiusY="12" />
            </GeometryDrawing.Geometry>
          </GeometryDrawing>
"@
        if ($b.Brush.StartsWith('<')) {
            $body = @"
$underlay
        <GeometryDrawing>
          <GeometryDrawing.Brush>
            $($b.Brush)
          </GeometryDrawing.Brush>
          <GeometryDrawing.Geometry>
            <PathGeometry Figures="$glyph" />
          </GeometryDrawing.Geometry>
        </GeometryDrawing>
"@
        }
        else {
            $body = @"
$underlay
        <GeometryDrawing Brush="$($b.Brush)" Geometry="$glyph" />
"@
        }
    }
    else {
        $shape = if ($b.Shape -eq 'rect') {
            '<RectangleGeometry Rect="0,0,24,24" RadiusX="7" RadiusY="7" />'
        }
        else {
            '<EllipseGeometry Center="12,12" RadiusX="12" RadiusY="12" />'
        }
        $brushXaml = if ($b.Brush.StartsWith('<')) {
            "        <GeometryDrawing.Brush>`n        $($b.Brush)`n        </GeometryDrawing.Brush>"
        }
        else {
            "        <GeometryDrawing Brush=`"$($b.Brush)`">"
        }
        $glyphXaml = @"
        <DrawingGroup>
          <DrawingGroup.Transform>
            <MatrixTransform Matrix="$($b.Scale),0,0,$($b.Scale),$($b.Tx),$($b.Tx)" />
          </DrawingGroup.Transform>
          <GeometryDrawing Brush="White" Geometry="$glyph" />
        </DrawingGroup>
"@
        if ($b.Brush.StartsWith('<')) {
            $body = @"
        <GeometryDrawing>
$brushXaml
          <GeometryDrawing.Geometry>
            $shape
          </GeometryDrawing.Geometry>
        </GeometryDrawing>
$glyphXaml
"@
        }
        else {
            $body = @"
$brushXaml
          <GeometryDrawing.Geometry>
            $shape
          </GeometryDrawing.Geometry>
        </GeometryDrawing>
$glyphXaml
"@
        }
    }
    Emit-Drawing $b.Key $body
}

$header = @"
<?xml version="1.0" encoding="utf-8"?>
<!--
  Official brand marks, converted from simple-icons SVG paths (CC0-1.0).
  Regenerate:  tools/convert-logos.ps1  (reads tools/logos/*.svg)
  Keys: Brand.{Whatsapp|Telegram|Instagram|Discord|Threads|X|Messenger|Reddit|Linkedin|Bluesky|Youtube}
  (keys must match PascalCase(Icon) = first char upper, rest as-is — same rule as Icon.* keys)
  Consumed by SidebarItem.IconImage; services without a brand key fall back to Icon.* glyphs.
-->
<ResourceDictionary xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
                    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
"@

$content = $header + "`n" + ($blocks -join "`n") + "`n`n</ResourceDictionary>`n"
Set-Content -Path $Out -Value $content -Encoding UTF8
Write-Output "BrandIcons written: $((Resolve-Path $Out).Path) ($($spec.Count) brands, $((Get-Item $Out).Length) bytes)"
