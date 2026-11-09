@echo off
rem Double-clic : Publie les liens de partage sur Internet (Tailscale Funnel, gratuit).
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0partage_public.ps1"
