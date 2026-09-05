# bvr installer — Beads Viewer in Rust (Windows)
# Usage: irm "https://raw.githubusercontent.com/quangdang46/beads_viewer_rust/main/install.ps1" | iex
$ErrorActionPreference = "Stop"

# === Config ===
$BinaryName = "bvr"
$BinaryExe  = "bvr.exe"
$Owner      = "quangdang46"
$Repo       = "beads_viewer_rust"
$Dest       = if ($env:DEST) { $env:DEST } else { Join-Path $env:USERPROFILE ".local\bin" }
$Version    = $env:VERSION
$EasyMode   = $false
$FromSource = $false
$MaxRetries = 3

function Log-Info($msg)    { Write-Host "[$BinaryName] $msg" }
function Log-Warn($msg)    { Write-Host "[$BinaryName] WARN: $msg" -ForegroundColor Yellow }
function Die($msg)         { Write-Host "ERROR: $msg" -ForegroundColor Red; exit 1 }

# === Args ===
foreach ($arg in $args) {
    switch ($arg) {
        "--easy-mode"   { $EasyMode = $true }
        "--from-source" { $FromSource = $true }
        default {}
    }
}

# === Platform (Split with limit 2 — windows_x86_64 must keep _64) ===
function Get-Platform {
    $arch = switch ($env:PROCESSOR_ARCHITECTURE) {
        "AMD64" { "x86_64" }
        "ARM64" { "aarch64" }
        default { Die "Unsupported arch: $env:PROCESSOR_ARCHITECTURE" }
    }
    return "windows_$arch"
}

# === Version resolution ===
function Resolve-Version {
    if ($Version) { return }
    try {
        $script:Version = (Invoke-RestMethod -Uri "https://api.github.com/repos/$Owner/$Repo/releases/latest" `
            -TimeoutSec 30).tag_name
    } catch {
        Die "Could not resolve latest version (no releases yet?). Use --from-source or install Rust and build."
    }
}

# === From source ===
function Build-FromSource {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Die "cargo not found — install Rust first: https://rustup.rs"
    }
    $src = Join-Path $env:TEMP "bvr-src-$(Get-Random)"
    git clone --depth 1 "https://github.com/$Owner/$Repo.git" $src
    Push-Location $src
    try {
        cargo build --release -p bv
        $bin = Join-Path $src "target\release\$BinaryExe"
        if (-not (Test-Path $bin)) { Die "Build finished but binary not found at $bin" }
        New-Item -ItemType Directory -Force -Path $Dest | Out-Null
        Copy-Item $bin (Join-Path $Dest $BinaryExe) -Force
    } finally { Pop-Location }
}

# === Main ===
if ($FromSource) {
    Build-FromSource
} else {
    Resolve-Version
    Log-Info "Latest release: $Version"
    $platform = Get-Platform
    $archive  = "$BinaryName-$Version-$platform.zip"
    $url      = "https://github.com/$Owner/$Repo/releases/download/$Version/$archive"
    $tmpZip   = Join-Path $env:TEMP "$archive"
    $tmpDir   = Join-Path $env:TEMP "bvr-extract-$(Get-Random)"

    $ok = $false
    for ($i = 1; $i -le $MaxRetries; $i++) {
        try {
            Invoke-WebRequest -Uri $url -OutFile $tmpZip -TimeoutSec 120 -ErrorAction Stop
            $ok = $true; break
        } catch { Log-Warn "Retry $i/$MaxRetries..."; Start-Sleep 3 }
    }
    if (-not $ok) { Log-Warn "Download failed — building from source..."; Build-FromSource }
    else {
        Expand-Archive -Path $tmpZip -DestinationPath $tmpDir -Force
        $bin = Get-ChildItem -Path $tmpDir -Recurse -Filter $BinaryExe | Select-Object -First 1
        if (-not $bin) { Die "Binary not found after extract" }
        New-Item -ItemType Directory -Force -Path $Dest | Out-Null
        Copy-Item $bin.FullName (Join-Path $Dest $BinaryExe) -Force
    }
}

# === PATH ===
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (($userPath -split ";") -notcontains $Dest) {
    if ($EasyMode) {
        [Environment]::SetEnvironmentVariable("Path", "$Dest;$userPath", "User")
        Log-Warn "PATH updated — restart your terminal."
    } else {
        Log-Warn "Add to PATH manually: $Dest"
    }
}

Write-Host ""
Write-Host ("✓ {0} installed → {1}" -f $BinaryName, (Join-Path $Dest $BinaryExe)) -ForegroundColor Green
Write-Host "  Quick start: cd your-beads-project && bvr"
