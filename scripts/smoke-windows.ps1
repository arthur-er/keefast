# Installs the keefast MSI, registers its native host as the app does, uninstalls it, and checks
# that the app and the registration are gone while another program's registration is left alone.
#
#   scripts/smoke-windows.ps1 dist\keefast-<version>-windows-x86_64.msi
#
# It changes the current user's install and browser registrations, so run it on a throwaway
# machine such as a CI runner.
param([Parameter(Mandatory)] [string] $Msi)

$ErrorActionPreference = 'Stop'
$Msi = (Resolve-Path $Msi).Path
$hostName = 'org.keepassxc.keepassxc_browser'
$installDir = Join-Path $env:LOCALAPPDATA 'Programs\keefast'
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'keefast.lnk'
$chromeHost = "Software\Google\Chrome\NativeMessagingHosts\$hostName"
$edgeHost = "Software\Microsoft\Edge\NativeMessagingHosts\$hostName"
$temp = [IO.Path]::GetTempPath()
$user = [Microsoft.Win32.Registry]::CurrentUser

function Invoke-Msiexec([string] $action, [string] $log) {
    $process = Start-Process msiexec.exe -Wait -PassThru `
        -ArgumentList $action, "`"$Msi`"", '/qn', '/l*v', "`"$log`""
    if ($process.ExitCode -ne 0) {
        Get-Content $log -Tail 80
        throw "msiexec $action exited with $($process.ExitCode)"
    }
}

# UTF-8 without a byte order mark, as browsers and keefast read manifests.
function Write-Manifest([string] $path, [Parameter(ValueFromPipeline)] [string] $json) {
    [IO.File]::WriteAllText($path, $json)
}

function Register([string] $key, [string] $manifest) {
    $registered = $user.CreateSubKey($key)
    $registered.SetValue('', $manifest)
    $registered.Close()
}

function Registered([string] $key) {
    $registered = $user.OpenSubKey($key)
    if (-not $registered) { return $null }
    try { $registered.GetValue('') } finally { $registered.Close() }
}

function Assert([bool] $condition, [string] $message) {
    if (-not $condition) { throw "smoke check failed: $message" }
    Write-Host "ok: $message"
}

Invoke-Msiexec '/i' (Join-Path $temp 'keefast-install.log')
Assert (Test-Path (Join-Path $installDir 'keefast.exe')) 'the app is installed'
Assert (Test-Path (Join-Path $installDir 'keefast-native-host.exe')) 'the native host is installed next to it'
Assert (Test-Path $shortcut) 'the Start menu has a shortcut'

# Browser integration turned on, as keefast writes it for a browser that has run: its manifest
# in keefast's data directory, pointing at the installed host, and Chrome's registry key
# pointing at the manifest.
$manifest = Join-Path $env:APPDATA "keefast\$hostName.json"
New-Item -ItemType Directory -Force (Split-Path $manifest) | Out-Null
@{
    name = $hostName
    path = Join-Path $installDir 'keefast-native-host.exe'
    type = 'stdio'
    allowed_origins = @('chrome-extension://oboonakemofpalcgghocfoadofidjkkk/')
} | ConvertTo-Json | Write-Manifest $manifest
Register $chromeHost $manifest

# KeePassXC registered under the same host name for Edge, which keefast must leave alone.
$keepassxc = Join-Path $temp "keepassxc-$hostName.json"
@{ name = $hostName; path = 'C:\Program Files\KeePassXC\keepassxc-proxy.exe'; type = 'stdio' } |
    ConvertTo-Json | Write-Manifest $keepassxc
Register $edgeHost $keepassxc

Invoke-Msiexec '/x' (Join-Path $temp 'keefast-uninstall.log')
Assert (-not (Test-Path $installDir)) 'the install directory is removed'
Assert (-not (Test-Path $shortcut)) 'the Start menu shortcut is removed'
Assert (-not (Test-Path 'HKCU:\Software\keefast\Installer')) "the installer's registry key is removed"
Assert ($null -eq (Registered $chromeHost)) "Chrome's registration of keefast's host is removed"
Assert (-not (Test-Path $manifest)) "keefast's host manifest is removed"
Assert ((Registered $edgeHost) -eq $keepassxc) "KeePassXC's registration for Edge is left alone"
