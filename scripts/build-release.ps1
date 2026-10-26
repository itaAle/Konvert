#Requires -Version 5.1
<#
.SYNOPSIS
    Release build of Konvert for Windows: NSIS installer plus its SHA-256 checksum.

.DESCRIPTION
    Every input is pinned: Rust toolchain (rust-toolchain.toml), Cargo.lock,
    package-lock.json (npm ci) and the FFmpeg archive (SHA-256 verified).

    1. Checks that the version is the same in package.json, Cargo.toml and tauri.conf.json.
    2. Installs the exact npm dependencies (npm ci).
    3. Installs the pinned FFmpeg build.
    4. Runs the privacy check, the third-party license check and every test.
    5. Builds the installer with local paths remapped, so the binaries contain
       neither the user name nor the project folder of the build machine.
    6. Verifies that konvert.exe contains no local path.
    7. Copies the installer to release/ and writes <installer>.sha256.

    Network is used only here, at build time (npm, FFmpeg archive, and the NSIS
    toolchain that Tauri downloads on the first build). The app never connects.

.PARAMETER SkipTests
    Skip Vitest and cargo test (the privacy and license checks always run).

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/build-release.ps1
#>
[CmdletBinding()]
param([switch]$SkipTests)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
$Steps = 7

function Write-Step([int]$n, [string]$text) {
    Write-Host ""
    Write-Host "==> [$n/$Steps] $text" -ForegroundColor Cyan
}

function Invoke-Checked([string]$Exe, [string[]]$Arguments) {
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "'$Exe $($Arguments -join ' ')' failed with exit code $LASTEXITCODE" }
}

# Byte patterns (ANSI and UTF-16) of local paths that must not appear in binaries.
function Find-LocalPaths([string]$File, [string[]]$Needles) {
    $bytes = [IO.File]::ReadAllBytes($File)
    $latin1 = [Text.Encoding]::GetEncoding(28591).GetString($bytes)
    $utf16 = [Text.Encoding]::Unicode.GetString($bytes)
    @($Needles | Where-Object { $_ -and ($latin1.Contains($_) -or $utf16.Contains($_)) })
}

Push-Location $RepoRoot
try {
    Write-Step 1 'Version'
    $pkgVersion = (Get-Content -Raw package.json | ConvertFrom-Json).version
    $confVersion = (Get-Content -Raw src-tauri/tauri.conf.json | ConvertFrom-Json).version
    $cargoVersion = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
    if ($pkgVersion -ne $confVersion -or $pkgVersion -ne $cargoVersion) {
        throw "Version mismatch: package.json $pkgVersion, src-tauri/tauri.conf.json $confVersion, Cargo.toml $cargoVersion"
    }
    Write-Host "Konvert $pkgVersion"

    Write-Step 2 'npm dependencies (exact versions from package-lock.json)'
    Invoke-Checked npm @('ci', '--no-audit', '--no-fund')

    Write-Step 3 'FFmpeg (pinned build, SHA-256 verified)'
    Invoke-Checked powershell @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', 'scripts/setup-ffmpeg.ps1')

    Write-Step 4 'Checks and tests'
    Invoke-Checked node @('scripts/check-privacy.mjs')
    Invoke-Checked node @('scripts/generate-licenses.mjs', '--check')
    if (-not $SkipTests) {
        Invoke-Checked npx @('vitest', 'run')
        Invoke-Checked cargo @('test', '--workspace')
    }

    Write-Step 5 'Build (local paths remapped)'
    $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
    $rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE '.rustup' }
    $rustFlags = @(
        "--remap-path-prefix=$cargoHome=/cargo",
        "--remap-path-prefix=$rustupHome=/rustup",
        "--remap-path-prefix=$RepoRoot=/konvert",
        # Deterministic PE header (no link timestamp).
        '-Clink-arg=/Brepro'
    )
    # CARGO_ENCODED_RUSTFLAGS (0x1F-separated) keeps paths with spaces intact.
    $env:CARGO_ENCODED_RUSTFLAGS = $rustFlags -join [char]0x1f
    try {
        Invoke-Checked npx @('tauri', 'build')
    } finally {
        Remove-Item Env:CARGO_ENCODED_RUSTFLAGS -ErrorAction SilentlyContinue
    }

    Write-Step 6 'No local paths in the binaries'
    $needles = @($env:USERPROFILE, $RepoRoot, $cargoHome)
    if ($env:USERNAME -and $env:USERNAME.Length -ge 4) { $needles += "\$($env:USERNAME)\" }
    # @() because an empty result comes back as $null.
    $found = @(Find-LocalPaths (Join-Path $RepoRoot 'target/release/konvert.exe') $needles)
    if ($found.Count -gt 0) { throw "konvert.exe contains local paths: $($found -join ', ')" }
    Write-Host 'OK: konvert.exe contains no user name or local folder.'

    Write-Step 7 'Installer and checksum'
    $installer = Get-ChildItem -Path 'target/release/bundle/nsis' -Filter "*_${pkgVersion}_x64-setup.exe" |
        Sort-Object LastWriteTime | Select-Object -Last 1
    if (-not $installer) { throw 'Installer not found in target/release/bundle/nsis.' }
    $releaseDir = Join-Path $RepoRoot 'release'
    New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null
    $target = Join-Path $releaseDir $installer.Name
    Copy-Item -LiteralPath $installer.FullName -Destination $target -Force
    $hash = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant()
    # Same format as `sha256sum` ("<hash>  <file name>"), so `sha256sum -c` works too.
    [IO.File]::WriteAllText("$target.sha256", "$hash  $($installer.Name)`n")

    Write-Host ""
    Write-Host "Release ready in release/:" -ForegroundColor Green
    Write-Host "  $($installer.Name)  ($([math]::Round($installer.Length / 1MB, 1)) MB)"
    Write-Host "  $($installer.Name).sha256"
    Write-Host "  SHA-256: $hash"
} finally {
    Pop-Location
}
