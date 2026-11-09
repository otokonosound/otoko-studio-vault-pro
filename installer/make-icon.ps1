# Dessine l'icône d'OTOKO (le logo de la barre du haut : carré braise, anneau sombre) et
# l'écrit en .ico à plusieurs tailles (images PNG dans le fichier, lu par Windows depuis Vista).
param([string]$Out = "$PSScriptRoot\otoko.ico")
Add-Type -AssemblyName System.Drawing

function Draw([int]$s) {
    $bmp = New-Object System.Drawing.Bitmap $s, $s
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.Clear([System.Drawing.Color]::Transparent)
    $r = [single]($s * 0.28)
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $m = [single]($s * 0.04); $w = [single]($s - 2 * $m)
    $path.AddArc($m, $m, 2 * $r, 2 * $r, 180, 90)
    $path.AddArc($m + $w - 2 * $r, $m, 2 * $r, 2 * $r, 270, 90)
    $path.AddArc($m + $w - 2 * $r, $m + $w - 2 * $r, 2 * $r, 2 * $r, 0, 90)
    $path.AddArc($m, $m + $w - 2 * $r, 2 * $r, 2 * $r, 90, 90)
    $path.CloseFigure()
    $rect = New-Object System.Drawing.RectangleF $m, $m, $w, $w
    $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush $rect, ([System.Drawing.Color]::FromArgb(255, 255, 106, 61)), ([System.Drawing.Color]::FromArgb(255, 255, 181, 71)), 45.0
    $g.FillPath($brush, $path)
    $d = [single]($s * 0.46); $o = [single](($s - $d) / 2)
    $pen = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(255, 23, 10, 6)), ([single][Math]::Max(1.5, $s * 0.096))
    $g.DrawEllipse($pen, $o, $o, $d, $d)
    $g.Dispose()
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    return ,$ms.ToArray()
}

$sizes = 256, 64, 48, 32, 16
$images = @($sizes | ForEach-Object { ,(Draw $_) })
$fs = [System.IO.File]::Create($Out)
$bw = New-Object System.IO.BinaryWriter $fs
$bw.Write([UInt16]0); $bw.Write([UInt16]1); $bw.Write([UInt16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]; $len = $images[$i].Length
    $dim = if ($s -ge 256) { 0 } else { $s }
    $bw.Write([byte]$dim); $bw.Write([byte]$dim); $bw.Write([byte]0); $bw.Write([byte]0)
    $bw.Write([UInt16]1); $bw.Write([UInt16]32); $bw.Write([UInt32]$len); $bw.Write([UInt32]$offset)
    $offset += $len
}
foreach ($img in $images) { $bw.Write($img) }
$bw.Close()
Write-Output "Icône : $Out"
