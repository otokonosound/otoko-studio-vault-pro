# Annule la bascule : le serveur mobile de Studio Vault reprend le port 443 et les liens publics
# (8443). Le serveur d'Otoko Studio Vault Pro continue de tourner a cote (8775, https sur 8444).

. (Join-Path $PSScriptRoot 'commun.ps1')
Admin-Requis $PSCommandPath
Journal 'retour-studio-vault'
trap { Write-Host ""; Write-Host "ERREUR : $_" -ForegroundColor Red; Fin; exit 1 }

Etape "Serveur mobile de Studio Vault"
if (-not (Get-ScheduledTask -TaskName 'Studio Vault mobile' -ErrorAction SilentlyContinue)) {
    throw "La tache 'Studio Vault mobile' n'existe pas sur ce PC."
}
Get-NetFirewallRule -DisplayName 'Studio Vault mobile*' -ErrorAction SilentlyContinue | Enable-NetFirewallRule
Enable-ScheduledTask -TaskName 'Studio Vault mobile' | Out-Null
Start-ScheduledTask -TaskName 'Studio Vault mobile'
$ok = $false
for ($i = 0; $i -lt 30 -and -not $ok; $i++) {
    Start-Sleep -Seconds 1
    try { Invoke-RestMethod 'http://127.0.0.1:8765/api/ping' -TimeoutSec 3 | Out-Null; $ok = $true } catch { }
}
if ($ok) { Write-Host "Studio Vault repond sur 8765." -ForegroundColor Green }
else { Write-Host "Studio Vault ne repond pas encore (journal : $env:APPDATA\ComposerVault\mobile_cache\serveur.log)." -ForegroundColor Yellow }

$ts = Trouver-Tailscale
if ($ts) {
    $carte = Lire-Serve $ts
    Etape "Tailscale : https://<mini>.ts.net -> Studio Vault, Otoko Studio Vault Pro sur 8444"
    Publier $ts 443 8765
    Publier $ts 8444 $PortPrive
    if ($carte.ContainsKey(8443) -and $carte[8443].Funnel) {
        Publier $ts 8443 8766 -Funnel
    }
    & $ts serve status
}
Remove-Item (Join-Path $env:APPDATA 'OTOKO\bascule.json') -ErrorAction SilentlyContinue
Write-Host ""
Write-Host "Retour fait : Studio Vault sert de nouveau l'app mobile et ses liens de partage." -ForegroundColor Green
Fin
