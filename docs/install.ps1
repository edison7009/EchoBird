# EchoBird (百灵鸟) — Windows Installer / Updater
# Usage:   irm https://echobird.ai/install.ps1 | iex
# License: MIT (https://github.com/edison7009/EchoBird/blob/main/LICENSE)

$ErrorActionPreference = "Stop"

# Force UTF-8 console output so any non-ASCII text (paths, error messages
# from native tools) renders correctly. Windows PowerShell 5.1 defaults
# its output to the local code page (often GBK or CP-1252), which mangles
# anything outside the active codepage into "ç¾çµé¸"-style mojibake.
try { [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new() } catch {}

Write-Host ""
Write-Host "  EchoBird Installer" -ForegroundColor Cyan
Write-Host "  ------------------" -ForegroundColor DarkGray

# Resolve version + Windows asset URL via GitHub Releases API.
# We hit api.github.com directly (the user's own anonymous quota is 60/h,
# plenty for a single install) so the script needs no companion server.
Write-Host "  Fetching latest version..." -ForegroundColor Gray
$latestVer = $null
$downloadUrl = $null
try {
    $release = Invoke-RestMethod "https://api.github.com/repos/edison7009/EchoBird/releases/latest" `
        -Headers @{ "User-Agent" = "EchoBird-Install" } -TimeoutSec 15
    # Match the CI rename in .github/workflows/release.yml — keep in sync.
    # Old releases (pre v3.7.9) used unsuffixed names; the OR pattern keeps
    # the script working if someone needs to re-install an older version.
    $winAsset = $release.assets | Where-Object {
        $_.name -like "*Windows_x64-setup.exe" -or $_.name -like "*_x64-setup.exe"
    } | Select-Object -First 1
    if ($winAsset) {
        $latestVer = $release.tag_name -replace '^v',''
        $downloadUrl = $winAsset.browser_download_url
    }
} catch {
    Write-Host "  GitHub API unreachable: $($_.Exception.Message)" -ForegroundColor Red
}

# Detect currently installed version from the Windows registry
$installedVer = $null
$regPaths = @(
    "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*",
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*",
    "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*"
)
foreach ($path in $regPaths) {
    $entry = Get-ItemProperty $path -ErrorAction SilentlyContinue |
             Where-Object { $_.DisplayName -like "EchoBird*" -or $_.DisplayName -like "*百灵鸟*" } |
             Select-Object -First 1
    if ($entry) {
        $installedVer = $entry.DisplayVersion
        break
    }
}

# Empty `version` = the Windows asset isn't out yet (CI probably still
# building for a just-tagged release). Show a friendly "come back later"
# message and pause so the window doesn't auto-close before the user
# can read it (some launch flows spawn a fresh PowerShell that exits
# the moment the script returns).
if (-not $latestVer) {
    Write-Host ""
    Write-Host "  A new version of EchoBird was just released." -ForegroundColor Yellow
    Write-Host "  The Windows installer is still uploading to GitHub." -ForegroundColor Yellow
    Write-Host "  Please try again in about 10 minutes." -ForegroundColor Yellow
    Write-Host ""
    if ($installedVer) {
        Write-Host "  Your current v$installedVer stays installed." -ForegroundColor Gray
        Write-Host ""
    }
    Write-Host "  Press any key to close..." -ForegroundColor DarkGray
    try { [void][System.Console]::ReadKey($true) } catch {}
    exit 0
}

Write-Host "  Latest    : v$latestVer" -ForegroundColor Green

if ($installedVer) {
    Write-Host "  Installed : v$installedVer" -ForegroundColor Gray
    if ($installedVer -eq $latestVer) {
        Write-Host ""
        Write-Host "  EchoBird is already up to date (v$installedVer)." -ForegroundColor Green
        Write-Host ""
        # Pause before exit. `irm ... | iex` from a Start-menu Run prompt
        # spawns a transient PowerShell window that closes the instant the
        # script returns — without this read the user never sees the
        # "already up to date" message. In a normal pwsh terminal this
        # just waits one keypress before handing back the prompt.
        Write-Host "  Press any key to continue..." -ForegroundColor DarkGray
        try { [void][System.Console]::ReadKey($true) } catch {}
        exit 0
    }
    Write-Host "  Upgrading v$installedVer  ->  v$latestVer ..." -ForegroundColor Yellow
} else {
    Write-Host "  Not installed - performing fresh install..." -ForegroundColor Gray
}

$out = "$env:TEMP\echobird-setup.exe"

Write-Host "  Downloading..." -ForegroundColor Gray
try {
    Invoke-WebRequest $downloadUrl -OutFile $out -UseBasicParsing
} catch {
    Write-Host ""
    Write-Host "  Download failed: $($_.Exception.Message)" -ForegroundColor Red
    Write-Host "  Manual download: https://github.com/edison7009/EchoBird/releases/latest" -ForegroundColor DarkYellow
    Write-Host ""
    exit 1
}

Write-Host "  Installing..." -ForegroundColor Gray
Start-Process $out -Wait

Write-Host ""
Write-Host "  Done! EchoBird v$latestVer installed." -ForegroundColor Green
Write-Host "  Launch it from the Start Menu." -ForegroundColor Gray
Write-Host ""
