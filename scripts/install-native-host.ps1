# Registers the LocalTrack native messaging host for Chrome (current user).
#
# Usage: powershell -ExecutionPolicy Bypass -File scripts\install-native-host.ps1 <extension-id>
param(
    [Parameter(Mandatory = $true)][string[]]$ExtensionIds
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$binary = Join-Path $root 'target\release\localtrack-native-host.exe'

if (-not (Test-Path $binary)) {
    Write-Host 'Building the native host...'
    Push-Location $root
    cargo build --release -p localtrack-native-host
    Pop-Location
}

& $binary install @ExtensionIds
Write-Host ''
Write-Host 'Reload the extension in chrome://extensions; the popup should show Connected.'
