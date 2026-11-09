# Publie sur Internet UNIQUEMENT le serveur des liens de partage (port local 8776), grace a
# Tailscale Funnel (gratuit, HTTPS automatique, aucun port a ouvrir sur la box). La bibliotheque
# complete reste privee. Pour tout couper : "Couper le partage public.bat".
# Port public : 8443 s'il est libre (ou deja a nous), sinon 10000 (Studio Vault garde 8443).

. (Join-Path $PSScriptRoot 'commun.ps1')
Admin-Requis $PSCommandPath
Journal 'partage-public'
trap { Write-Host ""; Write-Host "ERREUR : $_" -ForegroundColor Red; Fin; exit 1 }

$ts = Trouver-Tailscale
if (-not $ts) { throw "Tailscale introuvable." }

Etape "Serveur des partages"
if (-not (Serveur-Public-Pret)) {
    Start-ScheduledTask -TaskName 'OTOKO mobile' -ErrorAction SilentlyContinue
    for ($i = 0; $i -lt 20 -and -not (Serveur-Public-Pret); $i++) { Start-Sleep -Seconds 1 }
}
if (-not (Serveur-Public-Pret)) {
    throw "Le serveur des partages ne repond pas (127.0.0.1:$PortPublic). Lance d'abord 'Installer le serveur mobile.bat', ou active le serveur dans la page Mobile et partage."
}
Write-Host "Pret (127.0.0.1:$PortPublic)." -ForegroundColor Green

Etape "Publication sur Internet avec Tailscale Funnel"
$carte = Lire-Serve $ts
$port = $null
foreach ($p in 8443, 10000, 443) {
    if ($carte.ContainsKey($p) -and $carte[$p].Cible -eq $PortPublic -and $carte[$p].Funnel) { $port = $p; break }
}
if (-not $port) {
    foreach ($p in 8443, 10000, 443) { if (-not $carte.ContainsKey($p)) { $port = $p; break } }
    if (-not $port) { throw "Les trois ports publics de Funnel (443, 8443, 10000) sont deja pris." }
    Publier $ts $port $PortPublic -Funnel
}

Etape "Verification"
& $ts funnel status
$nom = Nom-Tailscale $ts
Write-Host ""
Write-Host "Partage public ACTIF. Tes liens commencent par :" -ForegroundColor Green
Write-Host "   https://${nom}:$port/s/..." -ForegroundColor Green
Write-Host "Cree-les dans la page Mobile et partage. Rien d'autre n'est visible depuis Internet."
Fin
