@echo off
rem Signe des executables (OTOKO, son hote de plugins, le programme d'installation) avec le
rem certificat de signature de code de l'artiste. Sans certificat, ne fait rien : Windows affichera
rem alors son avertissement a l'installation.
rem
rem   set OTOKO_SIGN_PFX=C:\chemin\certificat.pfx   et   set OTOKO_SIGN_PASSWORD=...   (fichier .pfx)
rem   ou set OTOKO_SIGN_SUBJECT=Nom du titulaire                   (certificat installe dans Windows,
rem                                                                   cle USB ou HSM compris)
rem Usage : sign.cmd fichier1.exe [fichier2.exe ...]
setlocal enabledelayedexpansion
if "%~1"=="" exit /b 0
if not defined OTOKO_SIGN_PFX if not defined OTOKO_SIGN_SUBJECT (
  echo Pas de certificat de signature : fichiers laisses non signes.
  exit /b 0
)
set "SIGNTOOL="
for /f "delims=" %%s in ('dir /b /s "%ProgramFiles(x86)%\Windows Kits\10\bin\signtool.exe" 2^>nul ^| findstr /i "\\x64\\"') do set "SIGNTOOL=%%s"
if not defined SIGNTOOL (
  echo signtool introuvable : installer le SDK Windows.
  exit /b 1
)
set "TS=/tr http://timestamp.digicert.com /td SHA256 /fd SHA256"
if defined OTOKO_SIGN_PFX (
  set "WHO=/f "%OTOKO_SIGN_PFX%""
  if defined OTOKO_SIGN_PASSWORD set "WHO=!WHO! /p "%OTOKO_SIGN_PASSWORD%""
) else (
  set "WHO=/n "%OTOKO_SIGN_SUBJECT%""
)
for %%f in (%*) do (
  "%SIGNTOOL%" sign !WHO! %TS% /d "OTOKO Studio & Live" "%%~f" || exit /b 1
)
exit /b 0