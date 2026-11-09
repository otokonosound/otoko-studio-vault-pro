# Ouvre le serveur mobile au Wi-Fi de la maison (la page Mobile et partage le propose).
# Sans mot de passe : tout appareil de ce reseau peut alors ecouter et telecharger ta musique.
# Pour revenir en arriere : "Refuser le Wi-Fi de la maison" sur la meme page.

. (Join-Path $PSScriptRoot 'commun.ps1')
Admin-Requis $PSCommandPath
Journal 'wifi-maison'
trap { Write-Host ""; Write-Host "ERREUR : $_" -ForegroundColor Red; Write-Host $_.InvocationInfo.PositionMessage; Fin; exit 1 }

Write-Host ""
Write-Host "ATTENTION" -ForegroundColor Yellow
Write-Host "Tout appareil connecte a ce reseau pourra ecouter et telecharger ta bibliothèque, sans mot de passe."
Write-Host "A reserver a un reseau ou tu connais tout le monde (pas un studio partage, une colocation, un hotel)."
Write-Host "Le telephone n'en a pas besoin pour marcher : Tailscale suffit. Le Wi-Fi sert surtout aux enceintes reseau."
Write-Host ""
$r = Read-Host "Autoriser le Wi-Fi de la maison ? (o/N)"
if ($r -notmatch '^[oOyY]') {
    Write-Host "Rien n'a change."
    Fin
    exit 0
}

Etape "Pare-feu : port $PortPrive ouvert sur ce reseau seulement"
Get-NetFirewallRule -DisplayName 'OTOKO mobile (Wi-Fi)' -ErrorAction SilentlyContinue | Remove-NetFirewallRule
# LocalSubnet + profil Prive : ni Internet, ni un reseau declare public dans Windows (hotel, cafe).
New-NetFirewallRule -DisplayName 'OTOKO mobile (Wi-Fi)' -Direction Inbound -Protocol TCP -LocalPort $PortPrive `
    -RemoteAddress LocalSubnet -Profile Private -Action Allow | Out-Null

$donnees = Join-Path $env:APPDATA 'OTOKO'
New-Item -ItemType Directory -Force -Path $donnees | Out-Null
[IO.File]::WriteAllText((Join-Path $donnees 'wifi_autorise.txt'), '1')
Write-Host "OK : le Wi-Fi de la maison est autorise (dans quelques secondes)." -ForegroundColor Green
Fin
