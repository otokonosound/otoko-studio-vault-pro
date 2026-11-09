# Rassemble la licence de chaque bibliotheque compilee dans le logiciel : MIT, BSD, Apache, MPL et les autres
# demandent que leur texte accompagne le programme. Sans reseau : tout vient du cache de cargo.
param([Parameter(Mandatory = $true)][string]$Sortie)
$ErrorActionPreference = 'Stop'
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
Set-Location (Split-Path -Parent $PSScriptRoot)

$meta = (cargo metadata --format-version 1 --offline --locked --filter-platform x86_64-pc-windows-msvc | Out-String) | ConvertFrom-Json
$arbre = cargo tree --workspace --edges normal --prefix none --format "{p}" --target x86_64-pc-windows-msvc --offline
$voulus = @{}
foreach ($l in $arbre) { $voulus[(($l -split ' ')[0..1] -join ' ')] = $true }

$texte = New-Object Text.StringBuilder
[void]$texte.AppendLine("Bibliotheques compilees dans Otoko Studio Vault Pro, et leurs licences.")
[void]$texte.AppendLine("")
$vus = @{}
$n = 0
foreach ($p in ($meta.packages | Sort-Object name, version)) {
    if (-not $p.source -or -not $voulus.ContainsKey("$($p.name) v$($p.version)")) { continue }
    $n++
    [void]$texte.AppendLine("=" * 78)
    [void]$texte.AppendLine("$($p.name) $($p.version)   Licence : $($p.license)")
    if ($p.repository) { [void]$texte.AppendLine("Sources : $($p.repository)") }
    $dir = Split-Path -Parent $p.manifest_path
    $fichiers = Get-ChildItem -LiteralPath $dir -File | Where-Object { $_.Name -match '^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE|COPYRIGHT)' }
    foreach ($f in $fichiers) {
        $contenu = Get-Content -LiteralPath $f.FullName -Raw -Encoding UTF8
        # Le meme texte (Apache 2.0 par exemple) n'est recopie qu'une fois.
        $cle = ($contenu -replace '\s+', ' ').Trim()
        if ($vus.ContainsKey($cle)) {
            [void]$texte.AppendLine("$($f.Name) : meme texte que pour $($vus[$cle]).")
        } else {
            $vus[$cle] = "$($p.name) $($p.version)"
            [void]$texte.AppendLine("--- $($f.Name)")
            [void]$texte.AppendLine($contenu.TrimEnd())
        }
    }
    [void]$texte.AppendLine("")
}
# Une liste vide ne doit jamais partir dans un installateur : on préfère que la construction échoue.
if ($n -lt 50) { throw "Seulement $n bibliotheques trouvees : la liste des licences est incomplete." }
[IO.File]::WriteAllText($Sortie, $texte.ToString(), (New-Object Text.UTF8Encoding($true)))
Write-Host "$n bibliotheques : $Sortie"
