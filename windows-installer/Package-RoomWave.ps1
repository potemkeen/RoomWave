[CmdletBinding()]
param([Parameter(Mandatory)][string]$VBCablePackage)
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot -Parent
$out=Join-Path $repo '.build-tmp\RoomWave-Signed-Setup'
$source=Join-Path $VBCablePackage 'vbaudio_cable64_win10.sys'
if ((Get-Content -LiteralPath (Join-Path $VBCablePackage 'vbMmeCable64_win10.inf') -Raw) -notmatch 'DriverVer\s*=.*3\.3\.1\.7') { throw 'Expected VB-CABLE Pack45, version 3.3.1.7.' }
if ((Get-AuthenticodeSignature -LiteralPath $source).Status -ne 'Valid') { throw 'VB-CABLE signature verification failed.' }
New-Item -ItemType Directory -Force $out,(Join-Path $out 'vbcable') | Out-Null
Copy-Item -Path (Join-Path $VBCablePackage '*') -Destination (Join-Path $out 'vbcable') -Recurse -Force
Copy-Item -LiteralPath (Join-Path $repo 'windows-host\src-tauri\target\release\roomwave-host.exe') -Destination $out -Force
foreach ($name in @('Install-RoomWave.ps1','Configure-VBCable.ps1','DriverSetup.cs','README.md')) { Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination $out -Force }
Write-Host $out
