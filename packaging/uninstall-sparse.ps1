# Removes the sparse package (native Windows 11 menu) and, unless -KeepClassic is given, restores the classic entries.
#   .\packaging\uninstall-sparse.ps1 [-InstallDir <folder with halo.exe>] [-KeepClassic]
param([string]$InstallDir, [switch]$KeepClassic)
. "$PSScriptRoot\common.ps1"

Get-AppxPackage $PackageName | Remove-AppxPackage
Get-ChildItem Cert:\CurrentUser\TrustedPeople | Where-Object { $_.Subject -eq $CertSubject } | Remove-Item
if (-not $KeepClassic) {
    & (Join-Path (Resolve-InstallDir $InstallDir) 'halo.exe') --context-menu on
}
Write-Host 'Sparse package removed.'
