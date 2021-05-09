#!/usr/bin/env pwsh
# Adapted from Deno's install script at https://github.com/denoland/deno_install/blob/main/install.ps1
# All rights reserved. MIT license.

$ErrorActionPreference = 'Stop'

if ((Get-WmiObject win32_operatingsystem | select osarchitecture).osarchitecture -notlike "64*") {
  throw "[dprint]: Only 64 bit operating systems are currently supported."
}

$Version = $args.Get(0)
$DprintZip = "dprint.zip"
$DprintExe = "dprint.exe"
$Target = 'x86_64-pc-windows-msvc'
$DprintUri = "https://github.com/dprint/dprint/releases/download/$Version/dprint-${Target}.zip"

# GitHub requires TLS 1.2
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

# download zip
Invoke-WebRequest $DprintUri -OutFile $DprintZip -UseBasicParsing

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
