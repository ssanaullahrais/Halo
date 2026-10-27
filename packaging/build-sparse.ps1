# Builds halo_menu.dll, copies it next to halo.exe, and produces a signed sparse package in packaging\out.
#   .\packaging\build-sparse.ps1 [-InstallDir <folder with halo.exe>] [-CertThumbprint <real code-signing cert>]
# Without -CertThumbprint a self-signed test certificate ("CN=Halo Dev") is created in the current user store.
param([string]$InstallDir, [string]$CertThumbprint)
. "$PSScriptRoot\common.ps1"

$install = Resolve-InstallDir $InstallDir
New-Item -ItemType Directory -Force $OutDir | Out-Null

# 1. COM DLL
Push-Location (Join-Path $Root 'shellext')
try {
    $ErrorActionPreference = 'Continue'   # cargo writes progress to stderr
    cmd /c 'cargo build --release 2>&1'
    if ($LASTEXITCODE) { throw 'cargo build failed' }
} finally { $ErrorActionPreference = 'Stop'; Pop-Location }
Copy-Item (Join-Path $Root 'shellext\target\release\halo_menu.dll') $install -Force
Write-Host "DLL copied to $install"

# 2. Certificate
if ($CertThumbprint) {
    $cert = Get-Item "Cert:\CurrentUser\My\$CertThumbprint"
} else {
    $cert = Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Subject -eq $CertSubject -and $_.NotAfter -gt (Get-Date).AddDays(30) } | Select-Object -First 1
    if (-not $cert) {
        $cert = New-SelfSignedCertificate -Type Custom -Subject $CertSubject -KeyUsage DigitalSignature `
            -FriendlyName 'Halo test signing' -CertStoreLocation Cert:\CurrentUser\My -KeyExportPolicy Exportable `
            -NotAfter (Get-Date).AddYears(3) `
            -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}')
        Write-Host "Created test certificate $($cert.Thumbprint)"
    }
}
if ($cert.Subject -ne $CertSubject) {
    Write-Warning "Certificate subject '$($cert.Subject)' differs from Publisher '$CertSubject' in AppxManifest.xml. Update the manifest Publisher to match."
}
Export-Certificate -Cert $cert -FilePath $CerPath -Force | Out-Null

# 3. Stage manifest + logos (the package only holds these; DLL and exe come from the external location)
$stage = Join-Path $OutDir 'stage'
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$stage\Assets" | Out-Null
Copy-Item "$PSScriptRoot\AppxManifest.xml" $stage
$icons = Join-Path $Root 'src-tauri\icons'
Copy-Item "$icons\Square44x44Logo.png" "$stage\Assets\Square44x44Logo.png"
Copy-Item "$icons\Square150x150Logo.png" "$stage\Assets\Square150x150Logo.png"
Copy-Item "$icons\StoreLogo.png" "$stage\Assets\StoreLogo.png"

# 4. Pack + sign
Remove-Item $MsixPath -Force -ErrorAction SilentlyContinue
& (Find-SdkTool 'makeappx.exe') pack /nv /o /d $stage /p $MsixPath
if ($LASTEXITCODE) { throw 'makeappx failed' }
& (Find-SdkTool 'signtool.exe') sign /fd SHA256 /sha1 $cert.Thumbprint $MsixPath
if ($LASTEXITCODE) { throw 'signtool failed' }
Copy-Item -LiteralPath $MsixPath -Destination (Join-Path $install 'HaloContextMenu.msix') -Force
Write-Host "Built $MsixPath (signed by $($cert.Subject))"


