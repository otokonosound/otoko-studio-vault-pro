@echo off
rem Construit l'app Android d'OTOKO (hors ligne, avec les outils Android deja installes pour Studio Vault).
rem Resultat : "..\Otoko Vault.apk", que le programme d'installation met a cote du logiciel : le telephone le
rem telecharge sur http://mon-pc:8775/otoko.apk (page Mobile et partage).
setlocal
set "OUTILS=%LOCALAPPDATA%\StudioVaultAndroid"
set "JAVA_HOME=%OUTILS%\jdk"
set "GRADLE_USER_HOME=%OUTILS%\gradle-home"
rem Java a besoin d'un dossier temporaire au chemin court sur ce PC
if not exist C:\svtmp mkdir C:\svtmp
set "TEMP=C:\svtmp"
set "TMP=C:\svtmp"
cd /d "%~dp0"
call "%OUTILS%\gradle-8.11.1\bin\gradle.bat" --offline --no-daemon --console=plain "-Dorg.gradle.jvmargs=-Xmx3g -Dfile.encoding=UTF-8 -Djdk.net.unixdomain.tmpdir=C:\svtmp" assembleRelease || exit /b 1
copy /y "app\build\outputs\apk\release\app-release.apk" "..\Otoko Vault.apk" >nul || exit /b 1
rem Le numero de version, a cote : le serveur le lit pour proposer la mise a jour au telephone.
for /f "tokens=3" %%v in ('findstr /c:"versionCode = " app\build.gradle.kts') do set "CODE=%%v"
>"..\Otoko Vault.version" echo %CODE%
echo APK pret : "%~dp0..\Otoko Vault.apk"
