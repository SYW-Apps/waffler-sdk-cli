# Waffler CLI installer — Windows (PowerShell)
# Usage: irm https://raw.githubusercontent.com/SYW-Apps/waffler-sdk-cli/main/install.ps1 | iex

$ErrorActionPreference = "Stop"

$Repo   = "SYW-Apps/waffler-sdk-cli"
$Bin    = "waffler.exe"
$Asset  = "waffler-x86_64-pc-windows-msvc.zip"
$InstallDir = "$env:LOCALAPPDATA\waffler\bin"

# ─── Fetch latest release ─────────────────────────────────────────────────────

Write-Host "→ Fetching latest release..." -ForegroundColor Cyan

$release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
$tag     = $release.tag_name
$url     = "https://github.com/$Repo/releases/download/$tag/$Asset"

Write-Host "→ Installing waffler $tag (x86_64-windows)..." -ForegroundColor Cyan

# ─── Download & extract ───────────────────────────────────────────────────────

$tmp = New-TemporaryFile | ForEach-Object { Remove-Item $_; New-Item -ItemType Directory -Path $_ }
$zip = Join-Path $tmp $Asset

Invoke-WebRequest $url -OutFile $zip
Expand-Archive $zip -DestinationPath $tmp

# ─── Install ──────────────────────────────────────────────────────────────────

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item (Join-Path $tmp $Bin) -Destination (Join-Path $InstallDir $Bin) -Force
Remove-Item $tmp -Recurse -Force

# ─── Add to PATH (user scope) ─────────────────────────────────────────────────

$currentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($currentPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("PATH", "$currentPath;$InstallDir", "User")
    Write-Host "  Added $InstallDir to your PATH." -ForegroundColor Yellow
    Write-Host "  Restart your terminal for the PATH change to take effect." -ForegroundColor Yellow
}

Write-Host ""
Write-Host "✓ waffler $tag installed to $InstallDir\$Bin" -ForegroundColor Green
Write-Host ""
Write-Host "  Run: waffler --help"
