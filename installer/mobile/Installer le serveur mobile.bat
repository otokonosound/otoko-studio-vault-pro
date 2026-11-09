@echo off
rem Double-clic : Installe le serveur de l'app mobile Otoko Vault (tache planifiee, pare-feu, Tailscale).
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0installer_serveur_mobile.ps1"
