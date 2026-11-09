# Outils communs aux scripts du serveur mobile (charge par les autres scripts).
# Serveur prive 8775 (Tailscale, et le Wi-Fi de la maison si on l'a autorise), partages 8776 (127.0.0.1, publie par Funnel).
# Studio Vault garde 8765 / 8766 tant qu'il n'est pas remplace.

$ErrorActionPreference = 'Stop'
$PortPrive = 8775
$PortPublic = 8776
$PortsStudioVault = @(8765, 8766)

function Etape($t) { Write-Host ""; Write-Host "== $t" -ForegroundColor Cyan }

# Relance le script en administrateur (pare-feu, tache planifiee, Tailscale).
function Admin-Requis($script) {
    $moi = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    if (-not $moi.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        Start-Process powershell "-NoProfile -ExecutionPolicy Bypass -File `"$script`"" -Verb RunAs
        exit
    }
}

function Journal($nom) {
    Start-Transcript -Path (Join-Path $env:TEMP "OTOKO-$nom.log") -Force | Out-Null
}

# otoko-serveur.exe : a cote du dossier des scripts (installation), et seulement la.
function Trouver-Serveur {
    $c = Join-Path (Split-Path -Parent $PSScriptRoot) 'otoko-serveur.exe'
    if (Test-Path $c) { return (Resolve-Path $c).Path }
    throw "otoko-serveur.exe introuvable (reinstalle le logiciel)."
}

function Trouver-Tailscale {
    $ts = (Get-Command tailscale -ErrorAction SilentlyContinue).Source
    if (-not $ts) { $ts = 'C:\Program Files\Tailscale\tailscale.exe' }
    if (Test-Path $ts) { return $ts }
    return $null
}

# Ce que publie Tailscale : port HTTPS -> @{ Cible = port local ; Funnel = public sur Internet }.
function Lire-Serve($ts) {
    $carte = @{}
    $json = (& $ts serve status --json 2>$null | Out-String).Trim()
    if (-not $json) { return $carte }
    $o = $json | ConvertFrom-Json
    if ($o.Web) {
        foreach ($p in $o.Web.PSObject.Properties) {
            $port = [int]($p.Name.Split(':')[-1])
            $proxy = $p.Value.Handlers.'/'.Proxy
            $cible = 0
            if ($proxy) { $cible = [int]($proxy.TrimEnd('/').Split(':')[-1]) }
            $funnel = $false
            if ($o.AllowFunnel) {
                $f = $o.AllowFunnel.PSObject.Properties | Where-Object { $_.Name -eq $p.Name }
                if ($f) { $funnel = [bool]$f.Value }
            }
            $carte[$port] = @{ Cible = $cible; Funnel = $funnel }
        }
    }
    if ($o.TCP) {
        foreach ($p in $o.TCP.PSObject.Properties) {
            $port = [int]$p.Name
            if (-not $carte.ContainsKey($port)) { $carte[$port] = @{ Cible = -1; Funnel = $false } }
        }
    }
    return $carte
}

function Nom-Tailscale($ts) {
    try { return ((& $ts status --json | ConvertFrom-Json).Self.DNSName).TrimEnd('.') } catch { return $null }
}

function Ping-Otoko {
    try { return Invoke-RestMethod "http://127.0.0.1:$PortPrive/api/ping" -TimeoutSec 5 } catch { return $null }
}

function Attendre-Otoko($secondes) {
    for ($i = 0; $i -lt $secondes; $i++) {
        $p = Ping-Otoko
        if ($p) { return $p }
        Start-Sleep -Seconds 1
    }
    return $null
}

function Serveur-Public-Pret {
    try { Invoke-WebRequest "http://127.0.0.1:$PortPublic/" -UseBasicParsing -TimeoutSec 3 | Out-Null; return $true }
    catch { return ($_.Exception.Response -and [int]$_.Exception.Response.StatusCode -eq 404) }
}

# Publie un port local sur Tailscale (prive) ou Funnel (Internet) en remplacant ce qui y etait.
function Publier($ts, $portHttps, $cible, [switch]$Funnel) {
    $carte = Lire-Serve $ts
    if ($carte.ContainsKey($portHttps)) {
        if ($carte[$portHttps].Funnel) { & $ts funnel "--https=$portHttps" off | Out-Null }
        else { & $ts serve "--https=$portHttps" off | Out-Null }
    }
    if ($Funnel) {
        Write-Host "Si Tailscale affiche un lien login.tailscale.com : ouvre-le, clique sur 'Enable' (gratuit), puis reviens ici."
        & $ts funnel --bg "--https=$portHttps" "http://127.0.0.1:$cible"
    } else {
        & $ts serve --bg "--https=$portHttps" "http://127.0.0.1:$cible"
    }
    if ($LASTEXITCODE -ne 0) { throw "tailscale a echoue (code $LASTEXITCODE)." }
}

function Retirer($ts, $portHttps) {
    $carte = Lire-Serve $ts
    if (-not $carte.ContainsKey($portHttps)) { return }
    if ($carte[$portHttps].Funnel) { & $ts funnel "--https=$portHttps" off | Out-Null }
    else { & $ts serve "--https=$portHttps" off | Out-Null }
}

function Fin {
    Stop-Transcript | Out-Null
    Write-Host ""
    pause
}
