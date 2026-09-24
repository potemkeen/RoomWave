# Validated only for standard VB-CABLE Pack45 / driver 3.3.1.7, not A/B/C/D or Voicemeeter.
[CmdletBinding()]
param([switch]$Apply, [string]$StateDirectory = "$env:ProgramData\RoomWave\Setup")
$ErrorActionPreference = 'Stop'
$devices = @(Get-PnpDevice -Class Media -ErrorAction Stop | Where-Object {
    (Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_HardwareIds -ErrorAction SilentlyContinue).Data -contains 'VBAudioVACWDM'
})
if ($devices.Count -ne 1) { throw 'Expected exactly one standard VB-CABLE device.' }
$version = (Get-PnpDeviceProperty -InstanceId $devices[0].InstanceId -KeyName DEVPKEY_Device_DriverVersion).Data
if ($version -ne '3.3.1.7') { throw "Unsupported VB-CABLE version: $version. Profile must be verified for this version first." }
$profile = [ordered]@{ VBAudioCableWDM_SR=48000; VBAudioCableWDM_Latency=3072; VBAudioCableWDM_LoopBack=1 }
$changes = @()
foreach ($view in @([Microsoft.Win32.RegistryView]::Registry64, [Microsoft.Win32.RegistryView]::Registry32)) {
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine, $view)
    try {
        $key = $base.OpenSubKey('SOFTWARE\VB-Audio\Cable')
        try {
            foreach ($name in $profile.Keys) {
                $exists = $null -ne $key -and $key.GetValueNames() -contains $name
                $old = if ($exists) { $key.GetValue($name) } else { $null }
                $kind = if ($exists) { $key.GetValueKind($name).ToString() } else { $null }
                if (!$exists -or $old -ne $profile[$name] -or $kind -ne 'DWord') {
                    $changes += [pscustomobject]@{view=$view.ToString();name=$name;existed=$exists;before=$old;kind=$kind;after=$profile[$name]}
                }
            }
        } finally { if ($key) { $key.Dispose() } }
    } finally { $base.Dispose() }
}
if ($Apply -and $changes.Count) {
    $admin = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    if (!$admin.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Applying the profile requires administrator privileges.' }
    New-Item -ItemType Directory -Path $StateDirectory -Force | Out-Null
    # Save every transaction BEFORE writing. Never overwrite the original settings on a rerun.
    $backup = Join-Path $StateDirectory ("vbcable-before-{0}.json" -f [guid]::NewGuid())
    @{version=$version;device=$devices[0].InstanceId;changes=$changes} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $backup -Encoding UTF8
    foreach ($change in $changes) {
        $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine, [Microsoft.Win32.RegistryView]::$($change.view))
        try {
            $key = $base.CreateSubKey('SOFTWARE\VB-Audio\Cable')
            try {
                $key.SetValue($change.name, [int]$change.after, [Microsoft.Win32.RegistryValueKind]::DWord)
                if ($key.GetValue($change.name) -ne $change.after) { throw "Readback failed: $($change.name)" }
            } finally { $key.Dispose() }
        } finally { $base.Dispose() }
    }
}
[pscustomobject]@{profileVersion=1;driverVersion=$version;applied=[bool]$Apply;changes=$changes;rebootRequired=([bool]$Apply -and $changes.Count -gt 0)}
