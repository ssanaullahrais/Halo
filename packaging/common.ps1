# Shared helpers for the sparse-package scripts. Dot-sourced; not run directly.
$ErrorActionPreference = 'Stop'

$Root        = Split-Path -Parent $PSScriptRoot
$OutDir      = Join-Path $PSScriptRoot 'out'
$PackageName = 'Halo.Wheel.ContextMenu'
$CertSubject = 'CN=Halo Dev'   # must equal Publisher in AppxManifest.xml
$MsixPath    = Join-Path $OutDir 'HaloContextMenu.msix'
$CerPath     = Join-Path $OutDir 'HaloDev.cer'

# Folder that holds halo.exe (and, after build, halo_menu.dll): the package's external location.
function Resolve-InstallDir([string]$InstallDir) {
    if ($InstallDir) { return (Resolve-Path $InstallDir).Path }
    foreach ($c in 'src-tauri\target\release', 'src-tauri\target\debug') {
        $p = Join-Path $Root $c
        if (Test-Path (Join-Path $p 'halo.exe')) { return $p }
    }
    throw 'halo.exe not found. Build Halo first, or pass -InstallDir <folder containing halo.exe>.'
}

function Find-SdkTool([string]$Name) {
    $hit = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\$Name" -ErrorAction SilentlyContinue |
        Sort-Object { [version]($_.FullName -split '\\')[-3] } -Descending | Select-Object -First 1
    if (-not $hit) { throw "$Name not found. Install the Windows 10/11 SDK." }
    $hit.FullName
}
