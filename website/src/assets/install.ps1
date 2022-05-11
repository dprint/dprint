#!/usr/bin/env pwsh
# Adapted from Deno's install script at https://github.com/denoland/deno_install/blob/main/install.ps1
# All rights reserved. MIT license.

$ErrorActionPreference = 'Stop'

if ($args.Length -gt 0) {
  $Version = $args.Get(0)
}

if ($PSVersionTable.PSEdition -ne 'Core') {
  $IsWindows = $true
  $IsMacOS = $false
}

$DprintInstall = $env:DPRINT_INSTALL
$BinDir = if ($DprintInstall) {
  "$DprintInstall\bin"
} elseif ($IsWindows) {
  "$Home\.dprint\bin"
}

$DprintZip = "$BinDir\dprint.zip"
$DprintExe = "$BinDir\dprint.exe"
$Target = 'x86_64-pc-windows-msvc'

# GitHub requires TLS 1.2
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$DprintUri = if (!$Version) {
  "https://github.com/dprint/dprint/releases/latest/download/dprint-${Target}.zip"
} else {
  "https://github.com/dprint/dprint/releases/download/$Version/dprint-${Target}.zip"
}

if (!(Test-Path $BinDir)) {
  New-Item $BinDir -ItemType Directory | Out-Null
}

# stop any running dprint editor services
Stop-Process -Name "dprint" -Erroraction 'silentlycontinue'

# download and install
Invoke-WebRequest $DprintUri -OutFile $DprintZip -UseBasicParsing

if (Get-Command Expand-Archive -ErrorAction SilentlyContinue) {
  Expand-Archive $DprintZip -Destination $BinDir -Force
} else {
  if (Test-Path $DprintExe) {
    Remove-Item $DprintExe
  }
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  [IO.Compression.ZipFile]::ExtractToDirectory($DprintZip, $BinDir)
}

Remove-Item $DprintZip

$User = [EnvironmentVariableTarget]::User
$Path = [Environment]::GetEnvironmentVariable('Path', $User)
if (!(";$Path;".ToLower() -like "*;$BinDir;*".ToLower())) {
  [Environment]::SetEnvironmentVariable('Path', "$Path;$BinDir", $User)
}
if (!(";$Env:Path;".ToLower() -like "*;$BinDir;*".ToLower())) {
  $Env:Path += ";$BinDir"
}
Write-Output "dprint was installed successfully to $DprintExe"
Write-Output "Run 'dprint --help' to get started"
