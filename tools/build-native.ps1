# Builds the optional native WhatsApp helper. Requires Go and a C compiler.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$dir = Join-Path $root 'native\whatsapp-bridge'
Push-Location $dir
try {
    $env:CGO_ENABLED = '1'
    go build -trimpath -ldflags '-s -w' -o United.WhatsAppBridge.exe .
    if ($LASTEXITCODE -ne 0) { throw "go build failed with exit code $LASTEXITCODE" }
    Get-Item United.WhatsAppBridge.exe | Select-Object FullName, Length, LastWriteTime
}
finally {
    Pop-Location
}
