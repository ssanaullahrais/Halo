# Registers the sparse package so Halo shows in the Windows 11 main right-click menu.
#   .\packaging\install-sparse.ps1 [-InstallDir <folder with halo.exe + halo_menu.dll>]
# Run build-sparse.ps1 first. Turns the classic "Show more options" entries off to avoid a duplicate menu.
param([string]$InstallDir)
. "$PSScriptRoot\common.ps1"

$install = Resolve-InstallDir $InstallDir
foreach ($f in 'halo.exe', 'halo_menu.dll') {
    if (-not (Test-Path (Join-Path $install $f))) { throw "$f missing in $install. Run build-sparse.ps1." }
}
if (-not (Test-Path $MsixPath)) { throw "$MsixPath missing. Run build-sparse.ps1." }

# Trust the test certificate. Windows only accepts sideloaded packages whose certificate is trusted for the whole
# machine, so this one step needs admin (one UAC prompt). A CA-issued code-signing certificate needs none of this.
$thumb = (New-Object System.Security.Cryptography.X509Certificates.X509Certificate2 $CerPath).Thumbprint
if (-not (Test-Path "Cert:\LocalMachine\TrustedPeople\$thumb")) {
    $cmd = "Import-Certificate -FilePath '$CerPath' -CertStoreLocation Cert:\LocalMachine\TrustedPeople | Out-Null"
    Start-Process powershell -Verb RunAs -Wait -ArgumentList '-NoProfile', '-Command', $cmd
    if (-not (Test-Path "Cert:\LocalMachine\TrustedPeople\$thumb")) { throw 'Certificate was not trusted (UAC declined?).' }
}

Get-AppxPackage $PackageName | Remove-AppxPackage
Add-AppxPackage -Path $MsixPath -ExternalLocation $install
Get-AppxPackage $PackageName | Format-List Name, Version, Status, InstallLocation, SignatureKind

# One menu only: remove the classic registry entries (settings.json is not touched).
& (Join-Path $install 'halo.exe') --context-menu off
Write-Host 'Done. If Halo is not in the right-click menu yet, restart Explorer (it closes open folder windows):'
Write-Host '  Stop-Process -Name explorer -Force'

