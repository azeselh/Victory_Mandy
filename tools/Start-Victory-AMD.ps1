<<<<<<< Updated upstream
﻿# Start-Victory-AMD.ps1 — Optimised launcher for AMD Radeon RX 7900 XT (RDNA 3)
# Branch: amd-enhanced-v2  |  Commit: a67a77b (AMD RDNA3 texture pipeline opt.)
=======
# Start-Victory-AMD.ps1 — Optimised launcher for AMD Radeon RX 7900 XT (RDNA 3)
# Branch: amd-enhanced-v2
>>>>>>> Stashed changes
#
# USAGE:
#   .\tools\Start-Victory-AMD.ps1 -TitleExecutable "C:\...\game_scarlett_master.exe"
#   .\tools\Start-Victory-AMD.ps1          # uses game_dir from victory.toml
#   .\tools\Start-Victory-AMD.ps1 -SkipWarmup  # skip shader pre-warm (not recommended)
#
# ENVIRONMENT VARIABLES SET:
#   VICTORY_LATE_WAIT=1       Async command submission (up to 256 in flight)
#   VICTORY_RELAY=2           Copy-back on its own thread + copy queue for card fetches
<<<<<<< Updated upstream
#   VICTORY_TEXTURE_STAGE_MS  400 ms default (overridden by this script if set)
#   VICTORY_TEXTURE_STAGE_MB  384 MB default (overridden by this script if set)
#   VICTORY_EARLY_TEXTURES_MS 25 ms default (overridden by this script if set)
#
# To profile a run with diagnostics:
#   $env:RUST_LOG="victory=debug"; .\tools\Start-Victory-AMD.ps1
#
# To revert texture defaults to upstream values:
#   $env:VICTORY_TEXTURE_STAGE_MS=100; $env:VICTORY_TEXTURE_STAGE_MB=192; $env:VICTORY_EARLY_TEXTURES_MS=12
=======
#   VICTORY_TEXTURE_STAGE_MS  100 ms default (preserves an existing override)
#   VICTORY_TEXTURE_STAGE_MB  192 MB default (preserves an existing override)
#   VICTORY_EARLY_TEXTURES_MS 12 ms default (preserves an existing override)
#
# To profile a run with diagnostics:
#   cargo build --profile diag -p victory --features diagnostics
#   .\tools\Start-Victory-AMD.ps1 -Diagnostics
# With -Executable, supply a binary built with the diagnostics feature.
>>>>>>> Stashed changes

param(
    [string]$TitleExecutable,
    [string]$Executable = "$PSScriptRoot/../target/release/victory.exe",
    [ValidateRange(1, 32)][int]$Jobs = 8,
    [switch]$SkipWarmup,
    [switch]$WarmOnly,
    [switch]$Diagnostics
)

$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath("$PSScriptRoot/..")
<<<<<<< Updated upstream
$exe  = (Resolve-Path -LiteralPath $Executable).Path
=======
if ($Diagnostics -and -not $PSBoundParameters.ContainsKey('Executable')) {
    $Executable = "$repo/target/diag/victory.exe"
    if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) {
        throw 'Build diagnostics first: cargo build --profile diag -p victory --features diagnostics'
    }
}
$exe  = (Resolve-Path -LiteralPath $Executable).Path
$diagnosticLog = $null
>>>>>>> Stashed changes

# ── AMD-specific environment ──────────────────────────────────────────────────
# Async command submission: reduces CPU blocking on Present/ExecuteCommandLists.
$env:VICTORY_LATE_WAIT = '1'

# Copy-back on its own thread + dedicated copy queue for card fetches.
# VICTORY_RELAY=2 is the best mode on AMD (copy queue avoids graphics-queue stalls).
$env:VICTORY_RELAY = '2'

<<<<<<< Updated upstream
# Texture upload pipeline — tuned for RDNA 3 / RX 7900 XT with 20 GB VRAM.
# At 2–4 FPS early-world the recording thread is idle most of the frame; giving
# heap_textures more time & bytes drains the 20k–44k slot backlog faster.
# These are now the compiled-in defaults but we set them explicitly so they
# appear in the diagnostics log and can be overridden per-run.
if (-not $env:VICTORY_TEXTURE_STAGE_MS) { $env:VICTORY_TEXTURE_STAGE_MS = '400' }
if (-not $env:VICTORY_TEXTURE_STAGE_MB) { $env:VICTORY_TEXTURE_STAGE_MB = '384' }
if (-not $env:VICTORY_EARLY_TEXTURES_MS) { $env:VICTORY_EARLY_TEXTURES_MS = '25' }
=======
# Texture staging runs on the CPU recording thread. More VRAM does not
# hide its cost: keep the ordinary time cap even when a backlog builds up.
if (-not $env:VICTORY_TEXTURE_STAGE_MS) { $env:VICTORY_TEXTURE_STAGE_MS = '100' }
if (-not $env:VICTORY_TEXTURE_STAGE_MB) { $env:VICTORY_TEXTURE_STAGE_MB = '192' }
if (-not $env:VICTORY_EARLY_TEXTURES_MS) { $env:VICTORY_EARLY_TEXTURES_MS = '12' }
>>>>>>> Stashed changes

# Wide merge: parallel merge of card-compare results (off by default; try =1
# if you have a fast CPU with many cores and the merge pass shows up in logs).
# $env:VICTORY_WIDE_MERGE = '1'

# Diagnostics mode: collect per-frame timing into %LOCALAPPDATA%\victory\diagnostics\
if ($Diagnostics) {
<<<<<<< Updated upstream
    $env:RUST_LOG = 'victory=debug'
    Write-Host '[AMD] Diagnostics mode on — logs will appear in %LOCALAPPDATA%\victory\diagnostics\' -ForegroundColor Cyan
=======
    $env:VICTORY_LOG = 'info'
    $run = Join-Path "$env:LOCALAPPDATA/victory/diagnostics" ((Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
    New-Item -ItemType Directory -Path $run | Out-Null
    $diagnosticLog = Join-Path $run 'stderr.log'
    Write-Host '[AMD] Diagnostics log:' $diagnosticLog -ForegroundColor Cyan
>>>>>>> Stashed changes
}

Write-Host '[AMD] VICTORY_LATE_WAIT  =' $env:VICTORY_LATE_WAIT  -ForegroundColor Green
Write-Host '[AMD] VICTORY_RELAY      =' $env:VICTORY_RELAY       -ForegroundColor Green
Write-Host '[AMD] TEXTURE_STAGE_MS   =' $env:VICTORY_TEXTURE_STAGE_MS  -ForegroundColor Green
Write-Host '[AMD] TEXTURE_STAGE_MB   =' $env:VICTORY_TEXTURE_STAGE_MB  -ForegroundColor Green
Write-Host '[AMD] EARLY_TEXTURES_MS  =' $env:VICTORY_EARLY_TEXTURES_MS -ForegroundColor Green

# ── Helper ────────────────────────────────────────────────────────────────────
function Invoke-Victory([string[]]$Arguments) {
    $previous = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
<<<<<<< Updated upstream
        & $exe @Arguments
=======
        if ($diagnosticLog) {
            & $exe @Arguments 2>> $diagnosticLog
        } else {
            & $exe @Arguments
        }
>>>>>>> Stashed changes
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    if ($exitCode -ne 0) { throw "Victory exited with code $exitCode." }
}

# ── Main ──────────────────────────────────────────────────────────────────────
Push-Location $repo
try {
    if (-not $TitleExecutable) {
        $config   = Invoke-Victory @('config')
        $gameLine = $config | Where-Object { $_ -match '^game_dir\s*:' } | Select-Object -First 1
        if (-not $gameLine) { throw 'Set game_dir in victory.toml or pass -TitleExecutable.' }
        $gameDir  = ($gameLine -replace '^game_dir\s*:\s*', '').Trim()
        if (-not (Test-Path -LiteralPath $gameDir -PathType Container)) {
            throw 'The configured game directory does not exist. Pass -TitleExecutable.'
        }
        $titles = @(Get-ChildItem -LiteralPath $gameDir -File -Filter '*.exe')
        if ($titles.Count -ne 1) { throw 'Pass -TitleExecutable to select the title executable.' }
        $TitleExecutable = $titles[0].FullName
    }
    $title = (Resolve-Path -LiteralPath $TitleExecutable).Path

    if (-not $SkipWarmup) {
        if (Get-Process victory -ErrorAction SilentlyContinue) {
            throw 'Close Victory before warming shaders and starting another run.'
        }
        $gameDir = Split-Path -Parent $title
        $sources = @($title)
        $sources += @(Get-ChildItem -LiteralPath $gameDir -File | Where-Object {
            $_.Extension -eq '.dll' -or $_.Name -match '(?i)shader.*\.(rpf|sgd5)$'
        } | ForEach-Object { $_.FullName })
        Write-Host '[AMD] Pre-warming shader cache with' $Jobs 'jobs (existing translations are reused).' -ForegroundColor Cyan
        Invoke-Victory (@('translate') + $sources + @('--warm', 'all', '--jobs', "$Jobs"))
    }

    if (-not $WarmOnly) {
        Write-Host '[AMD] Starting Victory — RDNA 3 optimised build (amd-enhanced-v2).' -ForegroundColor Cyan
        Write-Host '      First run: allow pipeline compilation to finish at the main menu.' -ForegroundColor Yellow
        Invoke-Victory @('run', $title, '--execute')
    }
} finally {
    Pop-Location
}
