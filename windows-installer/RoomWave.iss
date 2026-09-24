#ifndef Payload
  #error Payload is required
#endif
#ifndef RuntimeBootstrapper
  #error RuntimeBootstrapper is required
#endif
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
[Setup]
AppId={{E84645A0-1FF5-4C34-9759-94500391ECBC}
AppName=RoomWave
AppVersion={#AppVersion}
AppPublisher=RoomWave
DefaultDirName={autopf}\RoomWave
DisableDirPage=yes
DefaultGroupName=RoomWave
DisableProgramGroupPage=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
MinVersion=10.0.19041
OutputDir=..\dist\windows
OutputBaseFilename=RoomWave-Setup-{#AppVersion}-x64
SetupIconFile=..\windows-host\src-tauri\icons\icon.ico
UninstallDisplayIcon={app}\roomwave-host.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
InfoBeforeFile=SetupInfo.txt
CloseApplications=yes
RestartApplications=no
SetupLogging=yes

[Languages]
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Files]
Source: "{#Payload}\roomwave-host.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Payload}\roomwave-host.exe"; DestDir: "{tmp}\RoomWaveSetup"; Flags: dontcopy
Source: "{#Payload}\*.ps1"; DestDir: "{tmp}\RoomWaveSetup"; Flags: dontcopy
Source: "{#Payload}\DriverSetup.cs"; DestDir: "{tmp}\RoomWaveSetup"; Flags: dontcopy
Source: "{#Payload}\vbcable\*"; DestDir: "{tmp}\RoomWaveSetup\vbcable"; Flags: dontcopy recursesubdirs
Source: "{#Payload}\vbcable\VBCABLE_ControlPanel.exe"; DestDir: "{app}\VB-CABLE"; Flags: ignoreversion
Source: "{#Payload}\vbcable\readme.txt"; DestDir: "{app}\VB-CABLE"; Flags: ignoreversion
Source: "{#RuntimeBootstrapper}"; DestDir: "{tmp}\RoomWaveSetup"; DestName: "MicrosoftEdgeWebview2Setup.exe"; Flags: dontcopy

[Icons]
Name: "{group}\RoomWave"; Filename: "{app}\roomwave-host.exe"; WorkingDir: "{app}"
Name: "{group}\Удалить RoomWave"; Filename: "{uninstallexe}"

[Run]
Filename: "{app}\roomwave-host.exe"; Description: "Запустить RoomWave"; Flags: postinstall nowait skipifsilent runasoriginaluser; Check: CanLaunch

[Code]
var
  DriverRestart: Boolean;
  Prepared: Boolean;

function HasWebView: Boolean;
var Version: String;
begin
  Result := RegQueryStringValue(HKLM32, 'SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', 'pv', Version)
    and (Version <> '') and (Version <> '0.0.0.0');
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var Code: Integer; Params: String;
begin
  Result := '';
  if Prepared then exit;
  ExtractTemporaryFiles('{tmp}\RoomWaveSetup\*');
  if not HasWebView then begin
    if not Exec(ExpandConstant('{tmp}\RoomWaveSetup\MicrosoftEdgeWebview2Setup.exe'), '/silent /install', '', SW_HIDE, ewWaitUntilTerminated, Code) then begin
      Result := 'Не удалось запустить установку WebView2.'; exit;
    end;
    if (Code <> 0) and (Code <> 3010) then begin
      Result := 'Не удалось установить WebView2. Проверьте интернет и повторите установку. Код: ' + IntToStr(Code); exit;
    end;
    if not HasWebView then begin Result := 'WebView2 не найден после установки.'; exit; end;
    DriverRestart := Code = 3010;
  end;
  Params := '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + ExpandConstant('{tmp}\RoomWaveSetup\Install-RoomWave.ps1') +
    '" -ConfigureOnly -InstallDirectory "' + ExpandConstant('{app}') + '"';
  if not Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'), Params, '', SW_HIDE, ewWaitUntilTerminated, Code) then begin
    Result := 'Не удалось запустить настройку звука.'; exit;
  end;
  if (Code <> 0) and (Code <> 3010) then begin
    Result := 'Не удалось настроить звук. Закройте RoomWave и повторите установку. Подробности: C:\ProgramData\RoomWave\Setup\error.txt'; exit;
  end;
  DriverRestart := DriverRestart or (Code = 3010);
  Prepared := True;
end;

function NeedRestart(): Boolean;
begin Result := DriverRestart; end;

function CanLaunch(): Boolean;
begin Result := not DriverRestart; end;
