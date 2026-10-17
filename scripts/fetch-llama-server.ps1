# Windows: downloads llama.cpp's prebuilt CPU server and puts it, with the DLLs it loads, in
# src-tauri/binaries/llama/. The installer ships that folder as-is (see tauri.windows.conf.json);
# the backend finds llama-server.exe there. macOS and Linux build it from source instead
# (build-llama-server.sh).
$ErrorActionPreference = 'Stop'
$Version = 'b11205'
$Root = Split-Path -Parent $PSScriptRoot
$Out = Join-Path $Root 'src-tauri\binaries\llama'
$Stamp = Join-Path $Out 'llama-server.version'

if ((Test-Path (Join-Path $Out 'llama-server.exe')) -and (Test-Path $Stamp) -and ((Get-Content $Stamp) -eq $Version)) {
  Write-Host "llama-server $Version already downloaded"
  exit 0
}

$Zip = Join-Path ([IO.Path]::GetTempPath()) "llama-$Version-bin-win-cpu-x64.zip"
$Url = "https://github.com/ggml-org/llama.cpp/releases/download/$Version/llama-$Version-bin-win-cpu-x64.zip"
Write-Host "Downloading llama.cpp $Version for Windows"
Invoke-WebRequest -Uri $Url -OutFile $Zip -UseBasicParsing

$Unpacked = Join-Path ([IO.Path]::GetTempPath()) "llama-$Version"
if (Test-Path $Unpacked) { Remove-Item -Recurse -Force $Unpacked }
Expand-Archive -Path $Zip -DestinationPath $Unpacked

if (Test-Path $Out) { Remove-Item -Recurse -Force $Out }
New-Item -ItemType Directory -Force -Path $Out | Out-Null
# The server and every library it may load; the other tools in the zip aren't needed.
Copy-Item (Join-Path $Unpacked 'llama-server.exe') $Out
Get-ChildItem $Unpacked -Filter '*.dll' | Copy-Item -Destination $Out
Set-Content -Path $Stamp -Value $Version
Write-Host "llama-server $Version ready in $Out"
