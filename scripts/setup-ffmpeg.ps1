#Requires -Version 5.1
<#
.SYNOPSIS
    Downloads the pinned LGPL FFmpeg/FFprobe build, verifies its SHA-256 and
    installs it into src-tauri/binaries/ffmpeg (bundled by Tauri as a resource).

.DESCRIPTION
    Development/build-time only: the installed Konvert app never downloads anything.
    The archive URL and its SHA-256 are pinned in scripts/ffmpeg-manifest.json.
    A verified copy of the archive is cached in .ffmpeg-download/ (git-ignored).

.PARAMETER Force
    Download the archive again even if a verified copy is cached.

.PARAMETER Platform
    Manifest platform key. Default: windows-x86_64.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/setup-ffmpeg.ps1
#>
[CmdletBinding()]
param(
    [switch]$Force,
    [string]$Platform = 'windows-x86_64'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest is much faster without the progress bar
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$RepoRoot    = Split-Path -Parent $PSScriptRoot
$Manifest    = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'ffmpeg-manifest.json') | ConvertFrom-Json
$Property    = $Manifest.platforms.PSObject.Properties[$Platform]
if (-not $Property) { throw "Platform '$Platform' is not defined in scripts/ffmpeg-manifest.json." }
$Entry       = $Property.Value
if ($Entry.archive -ne 'zip') { throw "This script handles zip archives only; use scripts/setup-ffmpeg.sh for '$Platform'." }

$DownloadDir = Join-Path $RepoRoot '.ffmpeg-download'
$DestDir     = Join-Path $RepoRoot 'src-tauri/binaries/ffmpeg'
$FileName    = [IO.Path]::GetFileName(([Uri]$Entry.urls[0]).AbsolutePath)
$Archive     = Join-Path $DownloadDir $FileName
$Expected    = $Entry.sha256.ToLowerInvariant()

function Get-Sha256([string]$Path) {
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

New-Item -ItemType Directory -Force -Path $DownloadDir | Out-Null

# 1. Download (or reuse a cached copy whose hash matches).
if ($Force -or -not (Test-Path -LiteralPath $Archive) -or (Get-Sha256 $Archive) -ne $Expected) {
    $downloaded = $false
    foreach ($url in $Entry.urls) {
        Write-Host "Downloading $url"
        try {
            Invoke-WebRequest -Uri $url -OutFile $Archive -UseBasicParsing
            $downloaded = $true
            break
        } catch {
            Write-Warning "Download failed: $($_.Exception.Message)"
        }
    }
    if (-not $downloaded) {
        throw "Could not download the FFmpeg archive. If the pinned build was removed upstream, pick a new LGPL build and update scripts/ffmpeg-manifest.json."
    }
} else {
    Write-Host "Using cached archive $FileName"
}

# 2. Verify integrity before extracting anything.
$actual = Get-Sha256 $Archive
if ($actual -ne $Expected) {
    Remove-Item -LiteralPath $Archive -Force
    throw "SHA-256 mismatch for $FileName`n  expected: $Expected`n  actual:   $actual`nThe file was deleted. Do not use it."
}
Write-Host "SHA-256 verified: $actual"

# 3. Extract into a fresh, isolated directory.
$ExtractDir = Join-Path $DownloadDir ('extract-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $ExtractDir | Out-Null
try {
    $systemTar = Join-Path $env:SystemRoot 'System32\tar.exe'   # bsdtar: fast and handles zip
    if (Test-Path -LiteralPath $systemTar) {
        & $systemTar -xf $Archive -C $ExtractDir
        if ($LASTEXITCODE -ne 0) { throw "tar failed with exit code $LASTEXITCODE" }
    } else {
        Expand-Archive -LiteralPath $Archive -DestinationPath $ExtractDir
    }

    $TopDir = Get-ChildItem -LiteralPath $ExtractDir -Directory | Select-Object -First 1
    if (-not $TopDir) { throw 'Unexpected archive layout.' }
    $BinDir = Join-Path $TopDir.FullName $Entry.binDir

    # 4. Install only what Konvert needs (no ffplay, headers or import libraries).
    if (Test-Path -LiteralPath $DestDir) { Remove-Item -LiteralPath $DestDir -Recurse -Force }
    New-Item -ItemType Directory -Force -Path $DestDir | Out-Null
    foreach ($pattern in $Entry.files) {
        $found = @(Get-ChildItem -LiteralPath $BinDir -Filter $pattern -File)
        if ($found.Count -eq 0) { throw "No file matching '$pattern' in the archive." }
        $found | Copy-Item -Destination $DestDir
    }
    $license = Join-Path $TopDir.FullName 'LICENSE.txt'
    if (Test-Path -LiteralPath $license) { Copy-Item -LiteralPath $license -Destination (Join-Path $DestDir 'LICENSE.txt') }

    @(
        "FFmpeg build bundled with Konvert"
        "version:  $($Manifest.ffmpegVersion)"
        "license:  $($Manifest.license)"
        "archive:  $FileName"
        "sha256:   $Expected"
        "source:   $($Manifest.source.ffmpegCommit)"
        "recipe:   $($Manifest.source.buildRecipe)"
    ) | Set-Content -LiteralPath (Join-Path $DestDir 'BUILD-INFO.txt') -Encoding ASCII
} finally {
    Remove-Item -LiteralPath $ExtractDir -Recurse -Force -ErrorAction SilentlyContinue
}

# 5. Smoke test: the binaries run and provide every encoder Konvert relies on.
$ffmpeg  = Join-Path $DestDir 'ffmpeg.exe'
$ffprobe = Join-Path $DestDir 'ffprobe.exe'
# Capture the whole output first: `| Select-Object -First 1` would stop the
# pipeline early and make $LASTEXITCODE unreliable.
$versionOutput = @(& $ffmpeg -hide_banner -version)
if ($LASTEXITCODE -ne 0) { throw 'ffmpeg does not run.' }
$version = $versionOutput[0]
& $ffprobe -hide_banner -version | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'ffprobe does not run.' }

$encoders = (& $ffmpeg -hide_banner -encoders) -join "`n"
$missing = @($Manifest.requiredEncoders | Where-Object {
    $encoders -notmatch ("(?m)^\s*[VAS][A-Z.]{5}\s+" + [regex]::Escape($_) + "\s")
})
if ($missing.Count -gt 0) { throw "The FFmpeg build lacks required encoders: $($missing -join ', ')" }

Write-Host ""
Write-Host "OK: $version"
Write-Host "Installed into src-tauri/binaries/ffmpeg ($(@(Get-ChildItem -LiteralPath $DestDir -File).Count) files); all required encoders present."
