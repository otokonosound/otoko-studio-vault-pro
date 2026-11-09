# A lancer sur le PC principal (la page Mobile et partage le propose, ou double-clic sur
# "Installer le serveur mobile.bat"). Installe le serveur de l'app mobile Otoko Vault :
# - il demarre tout seul avec le PC, meme sans ouvrir de session (tache planifiee "OTOKO mobile") ;
# - il n'est joignable que depuis tes appareils Tailscale, jamais depuis Internet (le Wi-Fi de la maison
#   s'autorise a part : wifi_maison.ps1).
# Ne touche pas a Studio Vault (qui garde ses ports 8765 / 8766) ni a tes fichiers audio.

. (Join-Path $PSScriptRoot 'commun.ps1')
Admin-Requis $PSCommandPath
Journal 'installer-serveur-mobile'
trap { Write-Host ""; Write-Host "ERREUR : $_" -ForegroundColor Red; Write-Host $_.InvocationInfo.PositionMessage; Fin; exit 1 }

$exe = Trouver-Serveur
Write-Host "Serveur : $exe"

Etape "Pare-feu : port $PortPrive ouvert pour Tailscale"
Get-NetFirewallRule -DisplayName 'OTOKO mobile' -ErrorAction SilentlyContinue | Remove-NetFirewallRule
New-NetFirewallRule -DisplayName 'OTOKO mobile' -Direction Inbound -Protocol TCP -LocalPort $PortPrive `
    -RemoteAddress 100.64.0.0/10 -Action Allow | Out-Null
Write-Host "OK (le port $PortPublic des partages reste ferme : seul Tailscale Funnel y accede, sur cet ordinateur)."

Etape "Demarrage automatique"
$action = New-ScheduledTaskAction -Execute $exe -WorkingDirectory (Split-Path -Parent $exe)
$triggers = @((New-ScheduledTaskTrigger -AtStartup), (New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME))
$reglages = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable `
    -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew
# S4U : tourne sans session ouverte et sans enregistrer ton mot de passe.
$qui = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" -LogonType S4U -RunLevel Limited
$tache = New-ScheduledTask -Action $action -Trigger $triggers -Settings $reglages -Principal $qui `
    -Description "Serveur de l'app mobile Otoko Vault (bibliotheque et liens de partage, via Tailscale)"
Register-ScheduledTask -TaskName 'OTOKO mobile' -InputObject $tache -Force | Out-Null
Stop-ScheduledTask -TaskName 'OTOKO mobile' -ErrorAction SilentlyContinue
Start-ScheduledTask -TaskName 'OTOKO mobile'
Write-Host "Tache 'OTOKO mobile' creee et lancee."

Etape "Verification"
$p = Attendre-Otoko 20
if ($p) {
    Write-Host ("Serveur pret : {0} morceaux, {1} sorties." -f $p.tracks, $p.releases) -ForegroundColor Green
    if ($p.tracks -eq 0) { Write-Host "La base est vide : ajoute tes dossiers de musique depuis l'accueil du logiciel." -ForegroundColor Yellow }
} else {
    Write-Host "Le serveur ne repond pas encore. Journal : $env:APPDATA\OTOKO\mobile\serveur.log" -ForegroundColor Yellow
}

$ts = Trouver-Tailscale
if ($ts) {
    Etape "Adresse HTTPS privee (Tailscale)"
    $carte = Lire-Serve $ts
    $port = 443
    if ($carte.ContainsKey(443) -and $carte[443].Cible -ne $PortPrive) { $port = 8444 }   # 443 garde par Studio Vault
    if (-not ($carte.ContainsKey($port) -and $carte[$port].Cible -eq $PortPrive)) { Publier $ts $port $PortPrive }
    $nom = Nom-Tailscale $ts
    $https = if ($port -eq 443) { "https://$nom" } else { "https://${nom}:$port" }
    Write-Host "OK : $https (seulement pour tes appareils Tailscale)."
}

$nomPc = $env:COMPUTERNAME.ToLower()
Write-Host ""
Write-Host "Sur ton telephone (Tailscale connecte), dans Otoko Vault ou le navigateur :" -ForegroundColor Green
Write-Host "   http://${nomPc}:$PortPrive" -ForegroundColor Green
Fin
