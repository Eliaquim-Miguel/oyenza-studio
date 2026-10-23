; Poni installer (Inno Setup 6.3+).
;
; Build after `cargo build -p poni --release`:
;   iscc installer\poni.iss                      -> dist\Poni-Setup-0.1.0.exe
;   iscc /DAppVersion=0.2.0 installer\poni.iss   (another version)
;
; Per-user install: no admin rights and no UAC prompt. The app goes to
; %LOCALAPPDATA%\Programs\Poni; settings and data stay in %APPDATA%\Poni and
; survive uninstalling (only the cache in %LOCALAPPDATA%\Poni is removed).

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef ExeSource
  #define ExeSource "..\target\release\poni.exe"
#endif
#define AppName "Poni"
#define AppPublisher "Jatevo Labs"
#define AppExe "poni.exe"
; The island window's class (crates/poni/src/window.rs), used to ask a running copy to quit.
#define IslandClass "Poni.Island"

[Setup]
; Never change the AppId: upgrades and the uninstaller find the install by it.
AppId={{6F1D2B7A-3C84-4E59-A1D6-8B2E5F0C9A47}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppCopyright=Copyright (C) 2026 {#AppPublisher}
VersionInfoVersion={#AppVersion}
VersionInfoCompany={#AppPublisher}
VersionInfoDescription={#AppName} Setup
VersionInfoProductName={#AppName}
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\{#AppName}
DisableDirPage=yes
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\dist
OutputBaseFilename=Poni-Setup-{#AppVersion}
SetupIconFile=..\crates\poni\res\poni.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
; A running copy is asked to quit in PrepareToInstall; Restart Manager is the safety net.
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "autostart"; Description: "Start with Windows"; GroupDescription: "Startup:"

[Files]
Source: "{#ExeSource}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; Comment: "A living notch for Windows"

[Registry]
; Same value the app writes from its "Start with Windows" menu item (autostart.rs): the quoted exe path.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "{#AppName}"; ValueData: """{app}\{#AppExe}"""; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\{#AppExe}"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Make sure no copy is still running before its files are removed.
Filename: "{sys}\taskkill.exe"; Parameters: "/IM {#AppExe} /F"; Flags: runhidden waituntilterminated; RunOnceId: "StopPoni"

[UninstallDelete]
; Cache and log only (lyrics, poni.log); settings and data in %APPDATA%\Poni are kept.
Type: filesandordirs; Name: "{localappdata}\{#AppName}"

[Code]
const
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';
  WM_CLOSE = $0010;

{ Ask a running Poni to quit (it saves its data on the way out); force it if it doesn't within 3 s. }
procedure StopPoni();
var
  I, ResultCode: Integer;
begin
  if FindWindowByClassName('{#IslandClass}') = 0 then
    Exit;
  PostMessage(FindWindowByClassName('{#IslandClass}'), WM_CLOSE, 0, 0);
  for I := 1 to 30 do
  begin
    if FindWindowByClassName('{#IslandClass}') = 0 then
      Exit;
    Sleep(100);
  end;
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/IM {#AppExe} /F', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopPoni();
  Result := '';
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  { Unticking "Start with Windows" on a reinstall turns it off, even if the app's menu turned it on. }
  if (CurStep = ssPostInstall) and not WizardIsTaskSelected('autostart') then
    RegDeleteValue(HKEY_CURRENT_USER, RunKey, '{#AppName}');
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  { Runs before [UninstallRun], so a running copy gets the chance to save and quit cleanly. }
  if CurUninstallStep = usUninstall then
    StopPoni();
  { The app may have written the Run value itself; never leave it pointing at a removed exe. }
  if CurUninstallStep = usPostUninstall then
    RegDeleteValue(HKEY_CURRENT_USER, RunKey, '{#AppName}');
end;
