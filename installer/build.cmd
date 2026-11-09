@echo off
rem Construit le programme d'installation d'Otoko Studio Vault Pro : compile, prepare stage\, puis Inno Setup.
rem Resultat : ..\dist\Otoko-Studio-Vault-Pro-<version>-installation.exe (version : Cargo.toml).
setlocal
cd /d "%~dp0"
set "ISCC=%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe"
if not exist "%ISCC%" (
  echo Inno Setup 6 introuvable.
  exit /b 1
)
set "TARGET=%USERPROFILE%\otoko-build\otoko-target"
set "CARGO_TARGET_DIR=%TARGET%"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
for /f "tokens=3" %%v in ('findstr /b /c:"version = " ..\Cargo.toml') do set "VERSION=%%~v"
if "%VERSION%"=="" set "VERSION=0.1.0"

rem Les chemins de cette machine (nom d'utilisateur, dossier des sources) ne partent pas dans les programmes livres.
for %%d in ("%~dp0..") do set "SOURCES=%%~fd"
if "%USERPROFILE: =%"=="%USERPROFILE%" if "%SOURCES: =%"=="%SOURCES%" set "RUSTFLAGS=--remap-path-prefix=%USERPROFILE%=~ --remap-path-prefix=%SOURCES%=otoko -C link-arg=/PDBALTPATH:%%_PDB%%"

cargo build --release -p otoko -p otoko-serveur --manifest-path ..\Cargo.toml || exit /b 1

if exist stage rmdir /s /q stage
mkdir stage\licences
mkdir stage\mobile
copy /y "%TARGET%\release\otoko.exe" stage\ >nul || exit /b 1
copy /y "%TARGET%\release\otoko-serveur.exe" stage\ >nul || exit /b 1
copy /y mobile\*.ps1 stage\mobile\ >nul || exit /b 1
copy /y mobile\*.bat stage\mobile\ >nul || exit /b 1
rem L'app Android Otoko Vault (mobile\android\construire_apk.cmd) : servie au telephone sur /otoko.apk.
if exist "..\mobile\Otoko Vault.apk" copy /y "..\mobile\Otoko Vault.apk" stage\ >nul
if exist "..\mobile\Otoko Vault.version" copy /y "..\mobile\Otoko Vault.version" stage\ >nul
copy /y ..\docs\guide.html "stage\Guide Otoko Studio Vault Pro.html" >nul || exit /b 1
copy /y LICENCES.txt stage\ >nul || exit /b 1
copy /y ..\LICENSE stage\licences\GPL-3.0.txt >nul || exit /b 1
copy /y ..\apps\otoko\ui\fonts\OFL-*.txt stage\licences\ >nul
rem La licence et le nom de chaque bibliotheque compilee dans le logiciel.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0licences_tiers.ps1" "%~dp0stage\licences\Bibliotheques.txt" || exit /b 1

rem Signature (seulement si un certificat est indique : voir sign.cmd).
call "%~dp0sign.cmd" stage\otoko.exe stage\otoko-serveur.exe || exit /b 1
"%ISCC%" /Q /DAppVersion=%VERSION% otoko.iss || exit /b 1
call "%~dp0sign.cmd" "..\dist\Otoko-Studio-Vault-Pro-%VERSION%-installation.exe" || exit /b 1
rem L'empreinte a publier a cote de l'installateur (Releases) : le logiciel ne lance une mise a jour que si elle correspond.
rem GitHub donne aussi la sienne (digest) ; ce fichier sert si elle manque.
powershell -NoProfile -Command "$f='..\dist\Otoko-Studio-Vault-Pro-%VERSION%-installation.exe'; (Get-FileHash -Algorithm SHA256 $f).Hash.ToLower() + '  ' + (Split-Path $f -Leaf) | Set-Content -Encoding ascii ($f + '.sha256')" || exit /b 1
echo Programme d'installation : %~dp0..\dist\Otoko-Studio-Vault-Pro-%VERSION%-installation.exe