# Builds the United installer with Inno Setup.
# Usage:
#   .\tools\build-installer.ps1
#   .\tools\build-installer.ps1 -Iscc "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
param(
    [string]$Iscc = $env:ISCC
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent

if (-not $Iscc) {
    $candidates = @(
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 7\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 7\ISCC.exe"
    )
    $Iscc = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
}

if (-not $Iscc -or -not (Test-Path $Iscc)) {
    throw "ISCC.exe (Inno Setup) not found. Install Inno Setup or pass -Iscc <path> / set `$env:ISCC."
}

$publish = Join-Path $root 'build\United-win-x64'
$exe = Join-Path $publish 'United.exe'
if (-not (Test-Path $exe)) {
    throw "Published app not found at $publish. Run dotnet publish first."
}

Push-Location $root
try {
    & $Iscc 'installer\United.iss'
    if ($LASTEXITCODE -ne 0) { throw "ISCC failed with exit code $LASTEXITCODE" }
    Get-ChildItem 'build\United-Setup-*.exe' | Sort-Object LastWriteTime -Descending | Select-Object -First 1 FullName, Length, LastWriteTime
}
finally {
    Pop-Location
}
