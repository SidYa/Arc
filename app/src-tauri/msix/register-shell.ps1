# Registers Arc's sparse package (the Windows 11 Explorer context menu) for the current user.
# Run by the installer after copying the files (installer-hooks.nsh), with -Remove on uninstall.
# The package is signed with Arc's own certificate (arc.cer next to this script): trusting it
# needs administrator rights once (a UAC prompt); after that no rights are needed.
# Sets HKCU\Software\Arc\Menu\Packaged, so Arc drops its classic registry verbs (src/shell.rs).
param([string]$Dir = $PSScriptRoot, [switch]$Remove)

$ErrorActionPreference = 'Stop'
$name = 'Arc.Archiver'
$menuKey = 'HKCU:\Software\Arc\Menu'
$logDir = Join-Path $env:LOCALAPPDATA 'Arc'
New-Item -ItemType Directory -Force $logDir | Out-Null
$log = Join-Path $logDir 'shell-install.log'
function Write-Log([string]$message) { Add-Content -Path $log -Value "$(Get-Date -Format s) $message" -Encoding UTF8 }

function Set-Packaged([int]$value) {
    New-Item -Path $menuKey -Force -ErrorAction SilentlyContinue | Out-Null
    Set-ItemProperty -Path $menuKey -Name Packaged -Value $value -Type DWord
}

try {
    # A reinstall may come from another folder: always start clean
    Get-AppxPackage -Name $name | ForEach-Object {
        Write-Log "Removing $($_.PackageFullName)"
        Remove-AppxPackage -Package $_.PackageFullName
    }
    Set-Packaged 0
    if ($Remove) { exit 0 }

    # Windows 10 has no new menu: the classic registry verbs are enough
    if ([Environment]::OSVersion.Version.Build -lt 22000) {
        Write-Log 'Windows 10: classic menu only'
        exit 0
    }

    $cer = Join-Path $Dir 'arc.cer'
    $msix = Join-Path $Dir 'Arc.msix'
    $cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2 $cer
    if (-not (Test-Path "Cert:\LocalMachine\TrustedPeople\$($cert.Thumbprint)")) {
        Write-Log "Trusting the certificate $($cert.Thumbprint) (UAC)"
        $p = Start-Process certutil.exe -ArgumentList "-addstore TrustedPeople `"$cer`"" -Verb RunAs -WindowStyle Hidden -Wait -PassThru
        if ($p.ExitCode -ne 0) { throw "certutil exited with $($p.ExitCode)" }
    }

    # Writes Exe so the menu works before Arc's first start (Arc keeps it current afterwards)
    New-Item -Path $menuKey -Force -ErrorAction SilentlyContinue | Out-Null
    $menu = Get-ItemProperty -Path $menuKey
    if (-not $menu.Exe) { Set-ItemProperty -Path $menuKey -Name Exe -Value (Join-Path $Dir 'arc.exe') }
    if ($null -eq $menu.Enabled) { Set-ItemProperty -Path $menuKey -Name Enabled -Value 1 -Type DWord }

    Add-AppxPackage -Path $msix -ExternalLocation $Dir
    Set-Packaged 1
    # The classic verbs would show a second "Arc" under "Show more options" (Arc does the same on start)
    $classes = 'HKCU:\Software\Classes'
    $verbs = @("$classes\*\shell\Arc", "$classes\Directory\shell\Arc") +
        ('zip', '7z', 'rar', 'tar', 'gz', 'tgz', 'xz', 'txz' | ForEach-Object { "$classes\SystemFileAssociations\.$_\shell\Arc" })
    foreach ($key in $verbs) { Remove-Item -LiteralPath $key -Recurse -Force -ErrorAction SilentlyContinue }
    Write-Log "Registered from $Dir"
} catch {
    Write-Log "Failed: $_"
    exit 1
}
