<#
.SYNOPSIS
    VibeSSH setup launcher. Checks prerequisites, installs npm dependencies,
    and optionally builds the desktop installer.

.PARAMETER Build
    After setup, run `npm run tauri build` to produce the Windows installer
    (NSIS) under target/release/bundle (workspace-root target dir).

.PARAMETER Dev
    After setup, launch the app in dev mode (`npm run tauri dev`).

.EXAMPLE
    ./scripts/setup.ps1
    ./scripts/setup.ps1 -Dev
    ./scripts/setup.ps1 -Build
#>

param(
    [switch]$Build,
    [switch]$Dev
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

function Test-Command($name) {
    return [bool](Get-Command $name -ErrorAction SilentlyContinue)
}

Write-Host "== VibeSSH setup ==" -ForegroundColor Cyan

$missing = @()

if (-not (Test-Command "node")) { $missing += "Node.js (https://nodejs.org)" }
if (-not (Test-Command "npm"))  { $missing += "npm (bundled with Node.js)" }
if (-not (Test-Command "cargo")) { $missing += "Rust toolchain (https://rustup.rs)" }

if ($missing.Count -gt 0) {
    Write-Host "Missing prerequisites:" -ForegroundColor Yellow
    $missing | ForEach-Object { Write-Host "  - $_" -ForegroundColor Yellow }
    Write-Host ""
    Write-Host "Install Rust with:" -ForegroundColor Yellow
    Write-Host "  winget install Rustlang.Rustup" -ForegroundColor DarkYellow
    Write-Host "Then restart this terminal and re-run this script." -ForegroundColor Yellow
    exit 1
}

Write-Host "Node: $(node -v)"
Write-Host "npm:  $(npm -v)"
Write-Host "cargo: $(cargo --version)"

Push-Location $root
try {
    Write-Host "`nInstalling npm dependencies..." -ForegroundColor Cyan
    npm install

    if ($Build) {
        Write-Host "`nBuilding VibeSSH installer (this compiles Rust, can take a while)..." -ForegroundColor Cyan
        npm run tauri build
        $bundleDir = Join-Path $root "target\release\bundle"
        Write-Host "`nDone. Installer output:" -ForegroundColor Green
        Write-Host "  $bundleDir"
    }
    elseif ($Dev) {
        Write-Host "`nStarting VibeSSH in dev mode..." -ForegroundColor Cyan
        npm run tauri dev
    }
    else {
        Write-Host "`nSetup complete. Next steps:" -ForegroundColor Green
        Write-Host "  ./scripts/setup.ps1 -Dev     # run the app in dev mode"
        Write-Host "  ./scripts/setup.ps1 -Build   # build the Windows installer"
    }
}
finally {
    Pop-Location
}
