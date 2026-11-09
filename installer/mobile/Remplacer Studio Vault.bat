@echo off
rem Double-clic : Remplace le serveur mobile de Studio Vault par celui d'Otoko Studio Vault Pro (retour possible).
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0bascule_studio_vault.ps1"
