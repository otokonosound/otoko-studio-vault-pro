# Coupe l'acces Internet aux liens de partage (Tailscale Funnel). Les liens restent
# enregistres et remarcheront si tu relances "Activer le partage public.bat".
# Ne touche pas au partage public de Studio Vault.

. (Join-Path $PSScriptRoot 'commun.ps1')
Admin-Requis $PSCommandPath
Journal 'couper-partage-public'
trap { Write-Host ""; Write-Host "ERREUR : $_" -ForegroundColor Red; Fin; exit 1 }

$ts = Trouver-Tailscale
if (-not $ts) { throw "Tailscale introuvable." }
$carte = Lire-Serve $ts
$coupes = 0
foreach ($p in @($carte.Keys)) {
    if ($carte[$p].Cible -eq $PortPublic) { Retirer $ts $p; $coupes++ }
}
& $ts funnel status
Write-Host ""
if ($coupes -gt 0) { Write-Host "Partage public COUPE : ses liens ne sont plus accessibles depuis Internet." -ForegroundColor Green }
else { Write-Host "Le partage public n'etait pas actif." -ForegroundColor Green }
Fin
