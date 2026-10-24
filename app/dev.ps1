# Runs Arc in development mode (or builds the installer with -Build),
# making sure cargo and the Windows resource compiler are on PATH.
param([switch]$Build)
$ErrorActionPreference = 'Stop'

# Always put these first: a terminal opened before installing Rust has a stale PATH
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (-not (Test-Path "$cargoBin\cargo.exe")) {
    throw 'Rust is not installed: winget install Rustlang.Rustup'
}
$env:Path = "$cargoBin;$env:Path"

# rc.exe (icon and manifest of the exe) from the newest Windows SDK
$rc = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\rc.exe" -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending | Select-Object -First 1
if ($rc) { $env:Path = "$($rc.DirectoryName);$env:Path" }

# An elevated app cannot receive files dragged from Explorer (Windows UIPI)
$identity = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $Build -and $identity.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Warning 'Running as administrator: drag & drop from Explorer will not work. Start a normal terminal, or:'
    Write-Warning "  runas /trustlevel:0x20000 `"powershell -NoExit -ExecutionPolicy Bypass -File $PSCommandPath`""
}

Set-Location $PSScriptRoot
if (-not (Test-Path node_modules)) { npm install }
if ($Build) { npm run tauri build } else { npm run tauri dev }
