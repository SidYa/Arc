# Builds the Windows 11 context menu: arc_shell.dll (app/shellext) and the sparse package Arc.msix
# signed with Arc's own certificate, into src-tauri/shell (bundled by tauri.windows.conf.json).
# The certificate is created once in the developer's store (Cert:\CurrentUser\My); its private
# key never leaves it. arc.cer (public part) goes to the installer, which makes Windows trust it.
$ErrorActionPreference = 'Stop'
$app = Split-Path $PSScriptRoot
$tauri = Join-Path $app 'src-tauri'
$out = Join-Path $tauri 'shell'
New-Item -ItemType Directory -Force $out | Out-Null

$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if ($env:Path -notlike "*$cargoBin*") { $env:Path = "$cargoBin;$env:Path" }

# 1. The COM server (its own crate: static CRT, see shellext/.cargo/config.toml)
Push-Location (Join-Path $app 'shellext')
try {
    # Through cmd: cargo reports progress on stderr, which PowerShell 5.1 may treat as an error
    cmd /c "cargo build --release --quiet 2>&1"
    if ($LASTEXITCODE -ne 0) { throw 'shellext build failed' }
} finally { Pop-Location }
Copy-Item (Join-Path $app 'shellext\target\release\arc_shell.dll') $out -Force

# 2. Windows SDK tools
$sdk = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\makeappx.exe" -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending | Select-Object -First 1
if (-not $sdk) { throw 'Windows SDK not found (makeappx.exe, signtool.exe)' }
$makeappx = $sdk.FullName
$signtool = Join-Path $sdk.DirectoryName 'signtool.exe'

# 3. The package: manifest (version = the app's) and logos; no files of the app itself
#    (/nv: arc.exe and the DLL live in the external location, not in the package)
$manifest = [xml](Get-Content (Join-Path $tauri 'msix\AppxManifest.xml') -Raw -Encoding UTF8)
$publisher = $manifest.Package.Identity.Publisher
$version = (Get-Content (Join-Path $tauri 'tauri.conf.json') -Raw -Encoding UTF8 | ConvertFrom-Json).version
$manifest.Package.Identity.Version = "$version.0"
$layout = Join-Path ([IO.Path]::GetTempPath()) "arc-msix-$PID"
Remove-Item $layout -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $layout | Out-Null
try {
    $manifest.Save((Join-Path $layout 'AppxManifest.xml'))

    # Logos from the app icon. Settings and the menu look for exact sizes (targetsize-*,
    # unplated = without the accent-colored plate) through resources.pri, else show a generic icon.
    Add-Type -AssemblyName System.Drawing
    $assets = Join-Path $layout 'Assets'
    New-Item -ItemType Directory -Force $assets | Out-Null
    $icon = [Drawing.Image]::FromFile((Join-Path $tauri 'icons\icon.png'))
    function Save-Logo([string]$name, [int]$size) {
        $bitmap = New-Object Drawing.Bitmap $size, $size
        $g = [Drawing.Graphics]::FromImage($bitmap)
        $g.InterpolationMode = 'HighQualityBicubic'
        $g.DrawImage($icon, 0, 0, $size, $size)
        $bitmap.Save((Join-Path $assets "$name.png"), [Drawing.Imaging.ImageFormat]::Png)
        $g.Dispose(); $bitmap.Dispose()
    }
    try {
        foreach ($scale in 100, 200, 400) {
            Save-Logo "Square44x44Logo.scale-$scale" (44 * $scale / 100)
            Save-Logo "Square150x150Logo.scale-$scale" (150 * $scale / 100)
            Save-Logo "StoreLogo.scale-$scale" (50 * $scale / 100)
        }
        foreach ($size in 16, 20, 24, 30, 32, 36, 40, 48, 64, 96, 256) {
            Save-Logo "Square44x44Logo.targetsize-$size" $size
            Save-Logo "Square44x44Logo.targetsize-${size}_altform-unplated" $size
            Save-Logo "Square44x44Logo.targetsize-${size}_altform-lightunplated" $size
        }
    } finally { $icon.Dispose() }

    $makepri = Join-Path $sdk.DirectoryName 'makepri.exe'
    $priconfig = Join-Path ([IO.Path]::GetTempPath()) "arc-priconfig-$PID.xml"
    & $makepri createconfig /cf $priconfig /dq en-US /pv 10.0.0 /o | Out-Null
    & $makepri new /pr $layout /cf $priconfig /mn (Join-Path $layout 'AppxManifest.xml') /of (Join-Path $layout 'resources.pri') /o | Out-Null
    $priFailed = $LASTEXITCODE -ne 0
    Remove-Item $priconfig -ErrorAction SilentlyContinue
    if ($priFailed) { throw 'makepri failed' }

    $msix = Join-Path $out 'Arc.msix'
    & $makeappx pack /o /nv /d $layout /p $msix | Out-Null
    if ($LASTEXITCODE -ne 0) { & $makeappx pack /o /nv /d $layout /p $msix; throw 'makeappx failed' }
} finally { Remove-Item $layout -Recurse -Force -ErrorAction SilentlyContinue }

# 4. Sign with Arc's certificate (created on the first build)
$cert = Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Subject -eq $publisher -and $_.HasPrivateKey -and $_.NotAfter -gt (Get-Date) } |
    Sort-Object NotAfter -Descending | Select-Object -First 1
if (-not $cert) {
    Write-Host "Creating the package signing certificate $publisher"
    $cert = New-SelfSignedCertificate -Type Custom -Subject $publisher -KeyUsage DigitalSignature -FriendlyName 'Arc package signing' `
        -CertStoreLocation Cert:\CurrentUser\My -NotAfter (Get-Date).AddYears(20) `
        -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}')
}
& $signtool sign /q /fd SHA256 /sha1 $cert.Thumbprint /s My $msix
if ($LASTEXITCODE -ne 0) { throw 'signtool failed' }
Export-Certificate -Cert $cert -FilePath (Join-Path $out 'arc.cer') -Force | Out-Null
Write-Host "Context menu package: $msix"
