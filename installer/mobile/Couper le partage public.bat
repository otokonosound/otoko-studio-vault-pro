@echo off
rem Double-clic : Coupe l'acces Internet aux liens de partage.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0couper_partage_public.ps1"
