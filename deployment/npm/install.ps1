#!/usr/bin/env pwsh

$ErrorActionPreference = 'Stop'

$Version = $args.Get(0)
$DprintZip = "dprint.zip"
$DprintExe = "dprint.exe"
$Target = 'x86_64-pc-windows-msvc'
$DprintUri = "https://github.com/dprint/dprint/releases/download/$Version/dprint-${Target}.zip"

# GitHub requires TLS 1.2
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

# download zip
Invoke-WebRequest $DprintUri -OutFile $DprintZip -UseBasicParsing

# verify zip checksum
& node install_verify_checksum.js
if ($LASTEXITCODE -ne 0) { throw "[dprint]: Checksum verification failed." }

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
