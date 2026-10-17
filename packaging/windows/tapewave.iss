; Build with Inno Setup 6: ISCC /DAppVersion=0.2.0 tapewave.iss
#ifndef AppVersion
  #define AppVersion "0.2.0"
#endif

[Setup]
AppId={{A73BB663-448F-4A5F-A2EF-C88D30254624}
AppName=Tapewave
AppVersion={#AppVersion}
AppPublisher=Tapewave
AppPublisherURL=https://github.com/rahuling/tapewave
DefaultDirName={localappdata}\Programs\Tapewave
DefaultGroupName=Tapewave
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
UninstallDisplayIcon={app}\tapewave.exe
SetupIconFile=..\..\icons\icon.ico
LicenseFile=..\..\LICENSE
OutputDir=..\..\dist
OutputBaseFilename=Tapewave-{#AppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
AppMutex=Local\TapewaveNativePlayer
VersionInfoVersion={#AppVersion}
VersionInfoDescription=Tapewave Windows x64 Setup
UninstallDisplayName=Tapewave
; Keep one stable AppId for upgrades; no admin rights or firewall rules needed.

[Tasks]
Name: desktopicon; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "..\..\target\x86_64-pc-windows-msvc\release\tapewave.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Tapewave"; Filename: "{app}\tapewave.exe"
Name: "{group}\Uninstall Tapewave"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Tapewave"; Filename: "{app}\tapewave.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\tapewave.exe"; Description: "Launch Tapewave"; Flags: nowait postinstall skipifsilent

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  { Never delete user music. Preferences are kept unless the user opts out. }
  if (CurUninstallStep = usPostUninstall) and not UninstallSilent then
    if MsgBox('Also remove saved Tapewave preferences? Your music files will not be touched.', mbConfirmation, MB_YESNO) = IDYES then
    begin
      DeleteFile(ExpandConstant('{localappdata}\Tapewave\settings.json'));
      DeleteFile(ExpandConstant('{localappdata}\Tapewave\window.ron'));
    end;
end;
