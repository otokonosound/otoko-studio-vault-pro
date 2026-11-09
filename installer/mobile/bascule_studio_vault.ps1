# Remplace le serveur mobile de Studio Vault par celui d'Otoko Studio Vault Pro (a lancer quand tu as verifie
# que le logiciel te convient) :
# - la tache "Studio Vault mobile" est arretee et desactivee (rien n'est supprime) ;
# - https://<mini>.ts.net (port 443) mene a Otoko Studio Vault Pro ;
# - les liens de partage deja envoyes (port 8443) menent a Otoko Studio Vault Pro, qui a repris les memes jetons.
# Studio Vault, sa base et ses fichiers restent en place. Retour possible : "Revenir a Studio Vault.bat".

. (Join-Path $PSScriptRoot 'commun.ps1')
Admin-Requis $PSCommandPath
Journal 'bascule-studio-vault'
trap { Write-Host ""; Write-Host "ERREUR : $_" -ForegroundColor Red; Write-Host $_.InvocationInfo.PositionMessage; Fin; exit 1 }

Etape "Serveur mobile"
$p = Ping-Otoko
if (-not $p) {
    if (-not (Get-ScheduledTask -TaskName 'OTOKO mobile' -ErrorAction SilentlyContinue)) {
        throw "Le serveur mobile n'est pas installe : lance d'abord 'Installer le serveur mobile.bat'."
    }
    Start-ScheduledTask -TaskName 'OTOKO mobile'
    $p = Attendre-Otoko 20
}
if (-not $p) { throw "Le serveur mobile ne repond pas. Journal : $env:APPDATA\OTOKO\mobile\serveur.log" }
if ($p.tracks -eq 0) { throw "La base est vide : importe d'abord Studio Vault (Reglages > General)." }
Write-Host ("OK : {0} morceaux, {1} sorties." -f $p.tracks, $p.releases) -ForegroundColor Green

Etape "Arret du serveur mobile de Studio Vault"
if (Get-ScheduledTask -TaskName 'Studio Vault mobile' -ErrorAction SilentlyContinue) {
    Stop-ScheduledTask -TaskName 'Studio Vault mobile' -ErrorAction SilentlyContinue
    Disable-ScheduledTask -TaskName 'Studio Vault mobile' | Out-Null
}
Get-CimInstance Win32_Process -Filter "Name = 'pythonw.exe'" | Where-Object { $_.CommandLine -like '*composer_vault.mobile*' } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
Get-NetFirewallRule -DisplayName 'Studio Vault mobile*' -ErrorAction SilentlyContinue | Disable-NetFirewallRule
Write-Host "Tache 'Studio Vault mobile' desactivee (pas supprimee)."

$ts = Trouver-Tailscale
if ($ts) {
    $carte = Lire-Serve $ts
    Etape "Adresse HTTPS privee : https://<mini>.ts.net -> Otoko Studio Vault Pro"
    Publier $ts 443 $PortPrive
    if ($carte.ContainsKey(8444) -and $carte[8444].Cible -eq $PortPrive) { Retirer $ts 8444 }
    $public = @($carte.Keys | Where-Object { $carte[$_].Funnel })
    if ($public.Count -gt 0) {
        Etape "Liens de partage publics -> Otoko Studio Vault Pro"
        # Les liens de Studio Vault (port 8443) continuent de marcher : memes jetons.
        Publier $ts 8443 $PortPublic -Funnel
        foreach ($port in $public) { if ($port -ne 8443 -and $carte[$port].Cible -eq $PortPublic) { Retirer $ts $port } }
    }
    & $ts serve status
}

$fait = @{ date = (Get-Date).ToString('s'); de = 'Studio Vault'; vers = 'Otoko Studio Vault Pro' } | ConvertTo-Json
New-Item -ItemType Directory -Force (Join-Path $env:APPDATA 'OTOKO') | Out-Null
Set-Content -Path (Join-Path $env:APPDATA 'OTOKO\bascule.json') -Value $fait -Encoding UTF8

Write-Host ""
Write-Host "C'est fait : l'app mobile et les liens de partage passent par Otoko Studio Vault Pro." -ForegroundColor Green
Write-Host "Studio Vault reste installe ; 'Revenir a Studio Vault.bat' annule cette bascule."
Fin
