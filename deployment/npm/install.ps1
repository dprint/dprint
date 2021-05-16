#!/usr/bin/env pwsh

$ErrorActionPreference = 'Stop'

$Version = $args.Get(0)
$ExpectedZipChecksum = $args.Get(1)
$DprintZip = "dprint.zip"
$DprintExe = "dprint.exe"
$Target = 'x86_64-pc-windows-msvc'
$DprintUri = "https://github.com/dprint/dprint/releases/download/$Version/dprint-${Target}.zip"

# GitHub requires TLS 1.2
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

# download zip
Invoke-WebRequest $DprintUri -OutFile $DprintZip -UseBasicParsing

$ZipChecksum = (Get-FileHash $DprintZip -Algorithm SHA256).Hash

if ($ZipChecksum -ne $ExpectedZipChecksum) {
  Write-Error "Downloaded dprint zip checksum did not match the expected checksum (Actual: $ZipChecksum, Expected: $ExpectedZipChecksum)."
  exit 1
}

# extract executable
if (Get-Command Expand-Archive -ErrorAction SilentlyContinue) {
  Expand-Archive $DprintZip -Destination $PSScriptRoot -Force
} else {
  if (Test-Path $DprintExe) {
    Remove-Item $DprintExe
  }
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  [IO.Compression.ZipFile]::ExtractToDirectory($DprintZip)
}

# remove zip
Remove-Item $DprintZip
