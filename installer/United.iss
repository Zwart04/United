; United Messenger installer (Inno Setup).
; Build the app first:
;   dotnet publish src\United.App\United.App.csproj -c Release -r win-x64 --self-contained false -o build\United-win-x64
; Then compile this file with ISCC.exe. See tools\build-installer.ps1.

#define MyAppName "United Messenger"
#define MyAppVersion "0.2.3"
#define MyAppPublisher "United"
#define MyAppExeName "United.exe"
#define MyAppId "{{7E5B2C5A-3F1D-4B8E-9C2A-4D6F8A1B3C70}"

[Setup]
AppId={#MyAppId}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppSupportURL=https://github.com/Zwart04/United
AppUpdatesURL=https://github.com/Zwart04/United/releases
DefaultDirName={localappdata}\Programs\United Messenger
DefaultGroupName=United Messenger
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\build
OutputBaseFilename=United-Setup-{#MyAppVersion}
SetupIconFile=..\src\United.App\Assets\united.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
MinVersion=10.0.19041
CloseApplications=yes
RestartApplications=no
AppMutex=UnitedMessenger.SingleInstance
SetupLogging=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional icons:"; Flags: unchecked

[Files]
Source: "..\build\United-win-x64\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\United Messenger"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"
Name: "{autodesktop}\United Messenger"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch United Messenger"; Flags: nowait postinstall skipifsilent

[Code]
function HasDesktopRuntimeIn(Root: String): Boolean;
var
  FindRec: TFindRec;
begin
  Result := False;
  if FindFirst(Root + '\10.*', FindRec) then
  try
    repeat
      if (FindRec.Attributes and FILE_ATTRIBUTE_DIRECTORY) <> 0 then
        Result := True;
    until Result or not FindNext(FindRec);
  finally
    FindClose(FindRec);
  end;
end;

function HasDotNet10Desktop: Boolean;
var
  Names: TArrayOfString;
  I: Integer;
  Root: Integer;
begin
  Result := False;
  for Root := 0 to 1 do
  begin
    if Root = 0 then
    begin
      if not RegGetSubkeyNames(HKLM64,
        'SOFTWARE\dotnet\Setup\InstalledVersions\x64\sharedfx\Microsoft.WindowsDesktop.App', Names) then
        Continue;
    end
    else
    begin
      if not RegGetSubkeyNames(HKCU,
        'SOFTWARE\dotnet\Setup\InstalledVersions\x64\sharedfx\Microsoft.WindowsDesktop.App', Names) then
        Continue;
    end;
    for I := 0 to GetArrayLength(Names) - 1 do
      if Pos('10.', Names[I]) = 1 then
        Result := True;
  end;

  if Result then
    Exit;

  // dotnet-install.ps1 and the SDK installed in %LOCALAPPDATA% do not
  // necessarily write the sharedfx registry keys.
  Result :=
    HasDesktopRuntimeIn(ExpandConstant('{localappdata}\Microsoft\dotnet\shared\Microsoft.WindowsDesktop.App')) or
    HasDesktopRuntimeIn(ExpandConstant('{pf64}\dotnet\shared\Microsoft.WindowsDesktop.App')) or
    HasDesktopRuntimeIn(ExpandConstant('{pf32}\dotnet\shared\Microsoft.WindowsDesktop.App'));
end;

function HasWebView2In(Root: String): Boolean;
var
  FindRec: TFindRec;
begin
  Result := FileExists(Root + '\msedgewebview2.exe');
  if Result then
    Exit;

  // The Evergreen runtime normally lives in a versioned subfolder such as
  // Application\155.0.4283.60\msedgewebview2.exe.
  if FindFirst(Root + '\*', FindRec) then
  try
    repeat
      if (FindRec.Attributes and FILE_ATTRIBUTE_DIRECTORY) <> 0 then
        if FileExists(Root + '\' + FindRec.Name + '\msedgewebview2.exe') then
          Result := True;
    until Result or not FindNext(FindRec);
  finally
    FindClose(FindRec);
  end;
end;

function HasWebView2: Boolean;
begin
  Result :=
    HasWebView2In(ExpandConstant('{pf64}\Microsoft\EdgeWebView\Application')) or
    HasWebView2In(ExpandConstant('{pf32}\Microsoft\EdgeWebView\Application')) or
    HasWebView2In(ExpandConstant('{localappdata}\Microsoft\EdgeWebView\Application'));
end;

function InitializeSetup: Boolean;
var
  Answer: Integer;
begin
  Result := True;

  if not HasDotNet10Desktop then
  begin
    if WizardSilent then
    begin
      Result := False;
      Exit;
    end;

    Answer := MsgBox(
      'United Messenger needs the .NET 10 Desktop Runtime (x64).' + #13#10#13#10 +
      'Open the official download page now? Install it, then run this setup again.',
      mbError, MB_OKCANCEL);
    if Answer = IDOK then
      ShellExec('open', 'https://dotnet.microsoft.com/download/dotnet/10.0', '', '', SW_SHOWNORMAL, ewNoWait, Answer);
    Result := False;
    Exit;
  end;

  if not HasWebView2 then
  begin
    // Silent updates are started by an already-running United, so the runtime
    // is present even if detection missed a relocated/new layout. Never block
    // an unattended upgrade behind this warning.
    if WizardSilent then
      Exit;

    Answer := MsgBox(
      'Microsoft Edge WebView2 Runtime was not detected.' + #13#10#13#10 +
      'United uses the official WebView2 surface for the web services. ' +
      'You can install it from Microsoft, then continue.' + #13#10#13#10 +
      'Continue with setup anyway?',
      mbConfirmation, MB_YESNO);
    if Answer = IDNO then
    begin
      ShellExec('open', 'https://developer.microsoft.com/microsoft-edge/webview2/', '', '', SW_SHOWNORMAL, ewNoWait, Answer);
      Result := False;
    end;
  end;
end;
