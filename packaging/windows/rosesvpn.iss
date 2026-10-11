#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "dist\windows-stage"
#endif
#ifndef OutputDir
  #define OutputDir "dist"
#endif

[Setup]
AppId={{7D21786E-8B56-4E62-BE9D-4BCF675CE0B3}
AppName=RosesVPN
AppVersion={#AppVersion}
AppPublisher=Cattens-Roses
DefaultDirName={localappdata}\Programs\RosesVPN
DefaultGroupName=RosesVPN
UninstallDisplayName=RosesVPN
OutputDir={#OutputDir}
OutputBaseFilename=RosesVPN-Setup
SetupIconFile=..\..\assets\windows\rosesvpn.ico
UninstallDisplayIcon={app}\rosesvpn.exe
ArchitecturesInstallIn64BitMode=x64
PrivilegesRequired=lowest
DisableProgramGroupPage=yes
CloseApplications=yes
RestartApplications=no
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=no
Uninstallable=yes

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "{#SourceDir}\rosesvpn.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\resources-windows\*"; DestDir: "{app}\resources-windows"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\RosesVPN"; Filename: "{app}\rosesvpn.exe"
Name: "{autodesktop}\RosesVPN"; Filename: "{app}\rosesvpn.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\rosesvpn.exe"; Description: "Launch RosesVPN"; Flags: postinstall nowait skipifsilent
