[CmdletBinding()]
param(
    [string]$VBCablePackage,
    [string]$IsccPath,
    [string]$WebViewBootstrapper,
    [switch]$SkipHostBuild
)
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot -Parent
if (!$VBCablePackage) { $VBCablePackage=Join-Path $repo '.build-tmp\vbcable\package' }
if (!$IsccPath) { $IsccPath=Join-Path $repo '.build-tmp\installer-tools\Inno\ISCC.exe' }
if (!$WebViewBootstrapper) { $WebViewBootstrapper=Join-Path $repo '.build-tmp\installer-tools\MicrosoftEdgeWebview2Setup.exe' }
foreach ($file in @($IsccPath,$WebViewBootstrapper)) { if (!(Test-Path -LiteralPath $file)) { throw "Required build tool missing: $file. See windows-installer/README.md." } }
$signature=Get-AuthenticodeSignature $WebViewBootstrapper
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') { throw 'WebView2 bootstrapper signature is invalid.' }
$config=Get-Content (Join-Path $repo 'windows-host\src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json
$version=$config.version
if ($version -notmatch '^\d+\.\d+\.\d+$') { throw 'Expected a numeric three-part version.' }
if (!$SkipHostBuild) {
    Push-Location (Join-Path $repo 'windows-host')
    try { & npm.cmd run build:host; if ($LASTEXITCODE -ne 0) { throw 'Host build failed.' } } finally { Pop-Location }
}
& (Join-Path $PSScriptRoot 'Package-RoomWave.ps1') -VBCablePackage $VBCablePackage
$payload=Join-Path $repo '.build-tmp\RoomWave-Signed-Setup'
& $IsccPath "/DPayload=$payload" "/DRuntimeBootstrapper=$((Resolve-Path $WebViewBootstrapper).Path)" "/DAppVersion=$version" (Join-Path $PSScriptRoot 'RoomWave.iss')
if ($LASTEXITCODE -ne 0) { throw 'Installer compilation failed.' }
$exe=Join-Path $repo "dist\windows\RoomWave-Setup-$version-x64.exe"
$hash=(Get-FileHash $exe).Hash.ToLowerInvariant()
"$hash  $([IO.Path]::GetFileName($exe))" | Set-Content "$exe.sha256" -Encoding ascii
Write-Host "Built: $exe"
