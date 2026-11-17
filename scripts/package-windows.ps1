# Packages release builds of keefast, keefast-native-host and keefast-agent as an MSI, with WiX v5 on the PATH
# (`dotnet tool install --global wix --version 5.0.2`), and validates it. keefast.exe must be
# built with its web UI inside (the `custom-protocol` feature); it needs the WebView2 Runtime,
# which the MSI checks for.
#
#   pnpm --dir crates\app\ui install --frozen-lockfile; pnpm --dir crates\app\ui build
#   cargo build --release --locked -p keefast -p keefast-native-host -p keefast-agent --features keefast/custom-protocol
#   scripts/package-windows.ps1 target\release dist
#
# Writes keefast-<version>-windows-x86_64.msi into the output directory.
param(
    [Parameter(Mandatory)] [string] $BinDir,
    [Parameter(Mandatory)] [string] $OutDir
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.*)"$' |
    Select-Object -First 1).Matches.Groups[1].Value
if (-not $version) { throw 'no version in Cargo.toml' }
# An MSI's version is numbers only, so a pre-release gets its release's number, and the release
# upgrades it in place.
$msiVersion = $version -replace '[-+].*$', ''

New-Item -ItemType Directory -Force $OutDir | Out-Null
$msi = Join-Path $OutDir "keefast-$version-windows-x86_64.msi"
wix build (Join-Path $root 'packaging\windows\keefast.wxs') -arch x64 `
    -d "Version=$msiVersion" -d "BinDir=$((Resolve-Path $BinDir).Path)" `
    -d "IconFile=$(Join-Path $root 'crates\app\icons\icon.ico')" -o $msi
if ($LASTEXITCODE) { throw 'wix build failed' }
wix msi validate $msi
if ($LASTEXITCODE) { throw 'wix msi validate failed' }
