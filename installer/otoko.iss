; Programme d'installation d'Otoko Studio Vault Pro (Inno Setup 6).
; Construit par build.cmd, qui prépare le dossier stage\ et passe la version.
; Installation pour l'utilisateur seul par défaut (aucun droit d'administrateur demandé).
; L'AppId est celui d'OTOKO : une installation existante est mise à jour, dans son dossier.

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#define AppTitle "Otoko Studio Vault Pro"

[Setup]
AppId={{6F2C1E0B-7A44-4D2B-9C0E-0A7B2D6F3E11}
AppName={#AppTitle}
AppVersion={#AppVersion}
AppVerName={#AppTitle} {#AppVersion}
AppPublisher=Otoko
DefaultDirName={autopf}\{#AppTitle}
DefaultGroupName={#AppTitle}
DisableProgramGroupPage=yes
OutputDir=..\dist
OutputBaseFilename=Otoko-Studio-Vault-Pro-{#AppVersion}-installation
SetupIconFile=otoko.ico
UninstallDisplayIcon={app}\otoko.exe
UninstallDisplayName={#AppTitle}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ChangesAssociations=yes
; Le moins de questions possible : un dossier par défaut et pas de page récapitulative. Les licences restent installées.
DisableDirPage=yes
DisableReadyPage=yes
CloseApplications=yes

[Languages]
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"

[Tasks]
Name: "bureau"; Description: "Une icône sur le bureau"

[InstallDelete]
; Ce que laissait la version avec le studio.
Type: files; Name: "{app}\otoko-plughost.exe"
Type: files; Name: "{app}\OTOKO.apk"
Type: files; Name: "{app}\Guide OTOKO.html"
Type: files; Name: "{app}\mobile\Passer de Studio Vault a OTOKO.bat"
Type: files; Name: "{autoprograms}\OTOKO.lnk"
Type: files; Name: "{autoprograms}\Guide OTOKO.lnk"
Type: files; Name: "{autodesktop}\OTOKO.lnk"

[Files]
Source: "stage\otoko.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "stage\otoko-serveur.exe"; DestDir: "{app}"; Flags: ignoreversion
; Serveur mobile : scripts du Mini (tâche planifiée, partage public, passage depuis Studio Vault)
; et l'app Android Otoko Vault, que le serveur propose au téléphone.
Source: "stage\mobile\*"; DestDir: "{app}\mobile"; Flags: ignoreversion
Source: "stage\Otoko Vault.apk"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "stage\Otoko Vault.version"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "stage\Guide Otoko Studio Vault Pro.html"; DestDir: "{app}"; Flags: ignoreversion
Source: "stage\LICENCES.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "stage\licences\*"; DestDir: "{app}\licences"; Flags: ignoreversion recursesubdirs
Source: "otoko.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppTitle}"; Filename: "{app}\otoko.exe"; IconFilename: "{app}\otoko.ico"
Name: "{autoprograms}\Guide d'{#AppTitle}"; Filename: "{app}\Guide Otoko Studio Vault Pro.html"
Name: "{autodesktop}\{#AppTitle}"; Filename: "{app}\otoko.exe"; IconFilename: "{app}\otoko.ico"; Tasks: bureau

[Registry]
; La version avec le studio ouvrait les projets : ses associations de fichiers disparaissent.
Root: HKA; Subkey: "Software\Classes\.otoko"; ValueType: none; ValueName: ""; Flags: deletevalue dontcreatekey
Root: HKA; Subkey: "Software\Classes\OTOKO.Projet"; ValueType: none; Flags: deletekey dontcreatekey
Root: HKA; Subkey: "Software\Classes\.dawproject\OpenWithProgids"; ValueType: none; ValueName: "OTOKO.Projet"; Flags: deletevalue dontcreatekey
Root: HKA; Subkey: "Software\Classes\.mid\OpenWithProgids"; ValueType: none; ValueName: "OTOKO.Projet"; Flags: deletevalue dontcreatekey
Root: HKA; Subkey: "Software\Classes\.flp\OpenWithProgids"; ValueType: none; ValueName: "OTOKO.Projet"; Flags: deletevalue dontcreatekey
Root: HKA; Subkey: "Software\Classes\.als\OpenWithProgids"; ValueType: none; ValueName: "OTOKO.Projet"; Flags: deletevalue dontcreatekey

[Run]
; Une mise à jour arrête le serveur mobile de la tâche planifiée : on le relance (rien si la
; tâche n'existe pas).
Filename: "{sys}\schtasks.exe"; Parameters: "/Run /TN ""OTOKO mobile"""; Flags: runhidden skipifdoesntexist
Filename: "{app}\otoko.exe"; Description: "Lancer {#AppTitle}"; Flags: nowait postinstall skipifsilent
; Une mise à jour lancée depuis le logiciel est silencieuse : il se rouvre tout seul.
Filename: "{app}\otoko.exe"; Flags: nowait; Check: WizardSilent

[UninstallDelete]
; La base, ses sauvegardes et les réglages de l'utilisateur ne sont jamais supprimés
; (%APPDATA%\OTOKO reste en place).
Type: filesandordirs; Name: "{app}\licences"
Type: filesandordirs; Name: "{app}\mobile"

[Code]
// Le serveur mobile tourne sans fenêtre, sous la tâche planifiée, et le logiciel peut rester ouvert près de l'horloge :
// Windows ne sait pas toujours les fermer poliment et leurs fichiers resteraient pris. On les arrête avant la copie ;
// [Run] relance la tâche et le logiciel.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM otoko-serveur.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM otoko.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Result := '';
end;
