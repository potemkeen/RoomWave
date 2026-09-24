#Requires -RunAsAdministrator
[CmdletBinding()]
param([string]$PackageRoot, [string]$InstallDirectory="$env:ProgramFiles\RoomWave", [switch]$ConfigureOnly)
$ErrorActionPreference='Stop'
function Invoke-HostTool([string]$ToolArguments) {
    # Explicitly wait for GUI-subsystem executables in Windows PowerShell 5.
    $info=New-Object Diagnostics.ProcessStartInfo
    $info.FileName=$hostExe
    $info.Arguments=$ToolArguments
    $info.UseShellExecute=$false
    $info.CreateNoWindow=$true
    $info.RedirectStandardOutput=$true
    $info.RedirectStandardError=$true
    $info.StandardOutputEncoding=New-Object Text.UTF8Encoding($false)
    $info.StandardErrorEncoding=New-Object Text.UTF8Encoding($false)
    $process=[Diagnostics.Process]::Start($info)
    try {
        $stdout=$process.StandardOutput.ReadToEndAsync()
        $stderr=$process.StandardError.ReadToEndAsync()
        $process.WaitForExit()
        [pscustomobject]@{Code=$process.ExitCode;Output=$stdout.Result;Error=$stderr.Result}
    } finally { $process.Dispose() }
}
if (!$PackageRoot) { $PackageRoot=Split-Path -Parent $MyInvocation.MyCommand.Path }
$state=Join-Path $env:ProgramData 'RoomWave\Setup'
New-Item -ItemType Directory -Force $state | Out-Null
Start-Transcript -Path (Join-Path $state 'install.log') -Force | Out-Null
try {
    '' | Set-Content (Join-Path $state 'error.txt') -Encoding UTF8
    for ($attempt=0; $attempt -lt 10 -and (Get-Process roomwave-host -ErrorAction SilentlyContinue); $attempt++) { Start-Sleep -Milliseconds 500 }
    if (Get-Process roomwave-host -ErrorAction SilentlyContinue) { throw 'Close RoomWave before installation.' }
    if (![Environment]::Is64BitProcess -or $env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { throw 'Windows x64 and 64-bit PowerShell are required.' }
    @{installed=$false;status='in-progress'} | ConvertTo-Json | Set-Content (Join-Path $state 'result.json') -Encoding UTF8
    $hostExe=Join-Path $PackageRoot 'roomwave-host.exe'
    $driver=Join-Path $PackageRoot 'vbcable'
    $inf=Join-Path $driver 'vbMmeCable64_win10.inf'
    foreach ($path in @($hostExe,$inf,(Join-Path $PackageRoot 'DriverSetup.cs'))) {
        if (!(Test-Path -LiteralPath $path)) { throw "Package file missing: $path" }
    }
    $devices=@(Get-PnpDevice -Class Media | Where-Object {
        (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_HardwareIds -ErrorAction SilentlyContinue).Data -contains 'VBAudioVACWDM'
    })
    if ($devices.Count -gt 1) { throw 'More than one standard VB-CABLE device found.' }
    # Validate an existing version before changing any of its settings.
    if ($devices.Count -eq 1) { $null=& (Join-Path $PackageRoot 'Configure-VBCable.ps1') }
    $inspection=Invoke-HostTool '--inspect-audio'
    if ($inspection.Code -ne 0) { throw "Audio inspection failed: $($inspection.Error)" }
    $snapshot=$inspection.Output
    $null=($snapshot -join "`n") | ConvertFrom-Json
    $beforePath=Join-Path $state ("audio-before-{0}.json" -f [guid]::NewGuid())
    [IO.File]::WriteAllText($beforePath,($snapshot -join "`n"),(New-Object Text.UTF8Encoding($false)))
    $newDriver=$devices.Count -eq 0
    if ($newDriver) {
        $signature=Get-AuthenticodeSignature (Join-Path $driver 'vbaudio_cable64_win10.sys')
        if ($signature.Status -ne 'Valid') { throw 'VB-CABLE driver signature verification failed.' }
        Add-Type -Path (Join-Path $PackageRoot 'DriverSetup.cs')
        $null=[RoomWaveDriverSetup]::Install($inf)
    }
    # Wait for endpoint enumeration, bounded to 15 seconds on first install.
    $configured=$false
    try {
        for ($attempt=0; $attempt -lt 30; $attempt++) {
            $configureResult=Invoke-HostTool '--configure-vbcable'
            $configuration=$configureResult.Output
            if ($configureResult.Code -eq 0) { $configured=$true; break }
            if (!$newDriver) { break }
            Start-Sleep -Milliseconds 500
        }
    } finally {
        $restore=Invoke-HostTool ('--restore-install-output "'+$beforePath+'"')
        if ($restore.Code -ne 0) { throw "Could not restore pre-install Windows output: $($restore.Error)" }
    }
    if (!$configured) { throw 'Audio endpoint not ready. Restart Windows and run setup again; no manual audio configuration is needed.' }
    $profile=& (Join-Path $PackageRoot 'Configure-VBCable.ps1') -Apply -StateDirectory $state
    $profile | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $state 'profile-result.json') -Encoding UTF8
    $configuration | Set-Content (Join-Path $state 'endpoint-result.json') -Encoding UTF8
    if (!$ConfigureOnly) {
        New-Item -ItemType Directory -Force $InstallDirectory | Out-Null
        Copy-Item -LiteralPath $hostExe -Destination (Join-Path $InstallDirectory 'roomwave-host.exe') -Force
    }
    @{configured=$true;rebootRequired=($newDriver -or $profile.rebootRequired);loopbackAudioVerified=$false} | ConvertTo-Json | Set-Content (Join-Path $state 'result.json') -Encoding UTF8
    $exitCode=if ($newDriver -or $profile.rebootRequired) {3010} else {0}
} catch {
    $_ | Out-String | Set-Content (Join-Path $state 'error.txt') -Encoding UTF8
    @{configured=$false;error=$_.Exception.Message} | ConvertTo-Json | Set-Content (Join-Path $state 'result.json') -Encoding UTF8
    Write-Warning $_.Exception.Message
    $exitCode=1
} finally { Stop-Transcript | Out-Null }
exit $exitCode
