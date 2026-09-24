[CmdletBinding()]
param()
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot -Parent
$tools=Join-Path $repo '.build-tmp\installer-tools'
$cable=Join-Path $repo '.build-tmp\vbcable'
New-Item -ItemType Directory -Force $tools,$cable | Out-Null
function Get-VerifiedFile($Url,$Path,$Sha256) {
    if (!(Test-Path -LiteralPath $Path)) { Invoke-WebRequest -Uri $Url -OutFile $Path }
    if ((Get-FileHash -LiteralPath $Path).Hash -ne $Sha256) { throw "SHA256 mismatch: $Path. Review upstream changes before updating the pin." }
}
Get-VerifiedFile 'https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack45.zip' (Join-Path $cable 'pack45.zip') 'B950E39F01AF1D04EA623C8F6D8EB9B6EA5C477C637295FABF20631C85116BFB'
Expand-Archive -LiteralPath (Join-Path $cable 'pack45.zip') -DestinationPath (Join-Path $cable 'package') -Force
Get-VerifiedFile 'https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe' (Join-Path $tools 'innosetup.exe') '9C73C3BAE7ED48D44112A0F48E66742C00090BDB5BEF71D9D3C056C66E97B732'
$inno=Join-Path $tools 'Inno'
if (!(Test-Path (Join-Path $inno 'ISCC.exe'))) {
    $p=Start-Process (Join-Path $tools 'innosetup.exe') -WindowStyle Hidden -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/CURRENTUSER','/NOICONS',('/DIR="'+$inno+'"')) -Wait -PassThru
    if ($p.ExitCode -ne 0) { throw "Inno Setup installation failed: $($p.ExitCode)" }
}
# Evergreen bootstrapper intentionally follows Microsoft updates; validate publisher.
$webview=Join-Path $tools 'MicrosoftEdgeWebview2Setup.exe'
Invoke-WebRequest 'https://go.microsoft.com/fwlink/p/?LinkId=2124703' -OutFile $webview
$signature=Get-AuthenticodeSignature -LiteralPath $webview
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') { throw 'Invalid Microsoft WebView2 signature.' }
Get-FileHash (Join-Path $cable 'pack45.zip'),(Join-Path $tools 'innosetup.exe'),$webview
