# Start-Victory-AMD.ps1 — Optimised launcher for AMD Radeon RX 7900 XT (RDNA 3)
# Branch: amd-enhanced-v2
#
# USAGE:
#   .\tools\Start-Victory-AMD.ps1 -TitleExecutable "C:\...\game_scarlett_master.exe"
#   .\tools\Start-Victory-AMD.ps1          # uses game_dir from victory.toml
#   .\tools\Start-Victory-AMD.ps1 -SkipWarmup  # skip shader pre-warm (not recommended)
#
# ENVIRONMENT VARIABLES SET:
#   VICTORY_RELAY=2           Copy-back on its own thread + copy queue (auto Stage 3 late-wait)
#   VICTORY_COPY_PLAIN=1      In-process title memory copies with SEH fallback (cuts ZwReadVirtualMemory)
#   VICTORY_TEXTURE_STAGE_MS  100 ms default (preserves an existing override)
#   VICTORY_TEXTURE_STAGE_MB  192 MB default (preserves an existing override)
#   VICTORY_EARLY_TEXTURES_MS 12 ms default (preserves an existing override)
#
# To profile a run with diagnostics:
#   cargo build --profile diag -p victory --features diagnostics
#   .\tools\Start-Victory-AMD.ps1 -Diagnostics
# With -Executable, supply a binary built with the diagnostics feature.

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
if ($Diagnostics -and -not $PSBoundParameters.ContainsKey('Executable')) {
    $Executable = "$repo/target/diag/victory.exe"
    if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) {
        throw 'Build diagnostics first: cargo build --profile diag -p victory --features diagnostics'
    }
}
$exe = (Resolve-Path -LiteralPath $Executable).Path
$diagnosticLog = $null

# ── AMD-specific environment ──────────────────────────────────────────────────
# Copy-back on its own thread + dedicated copy queue for card fetches.
# Leaving VICTORY_LATE_WAIT unset lets VICTORY_RELAY=2 enable Stage 3 automatically
# (avoiding the Stage 1 synchronous Present/settle stalls).
if (-not $env:VICTORY_RELAY) { $env:VICTORY_RELAY = '2' }

# In-place memory copies reduce kernel syscall overhead substantially on Ryzen/Core CPUs.
if (-not $env:VICTORY_COPY_PLAIN) { $env:VICTORY_COPY_PLAIN = '1' }

# Texture staging runs on the CPU recording thread. More VRAM does not
# hide its cost: keep the ordinary time cap even when a backlog builds up.
if (-not $env:VICTORY_TEXTURE_STAGE_MS) { $env:VICTORY_TEXTURE_STAGE_MS = '100' }
if (-not $env:VICTORY_TEXTURE_STAGE_MB) { $env:VICTORY_TEXTURE_STAGE_MB = '192' }
if (-not $env:VICTORY_EARLY_TEXTURES_MS) { $env:VICTORY_EARLY_TEXTURES_MS = '12' }

# Optional / experimental switches (disabled by default):
#   $env:VICTORY_INDIRECT_WINDOW = '1'  # Faster indirect draws, occasional RX 7900 XT device loss seen under load
#   $env:VICTORY_WIDE_MERGE = '1'       # Parallel merge of card-compare results

# Diagnostics mode: collect per-frame timing into %LOCALAPPDATA%\victory\diagnostics\
if ($Diagnostics) {
    $env:VICTORY_LOG = 'info'
    $run = Join-Path "$env:LOCALAPPDATA/victory/diagnostics" ((Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
    New-Item -ItemType Directory -Path $run | Out-Null
    $diagnosticLog = Join-Path $run 'stderr.log'
    Write-Host '[AMD] Diagnostics log:' $diagnosticLog -ForegroundColor Cyan
}

$lateDisplay = if ($env:VICTORY_LATE_WAIT) { $env:VICTORY_LATE_WAIT } else { 'auto (stage 3)' }
Write-Host '[AMD] VICTORY_RELAY      =' $env:VICTORY_RELAY      -ForegroundColor Green
Write-Host '[AMD] VICTORY_LATE_WAIT  =' $lateDisplay            -ForegroundColor Green
Write-Host '[AMD] VICTORY_COPY_PLAIN =' $env:VICTORY_COPY_PLAIN -ForegroundColor Green
Write-Host '[AMD] TEXTURE_STAGE_MS   =' $env:VICTORY_TEXTURE_STAGE_MS  -ForegroundColor Green
Write-Host '[AMD] TEXTURE_STAGE_MB   =' $env:VICTORY_TEXTURE_STAGE_MB  -ForegroundColor Green
Write-Host '[AMD] EARLY_TEXTURES_MS  =' $env:VICTORY_EARLY_TEXTURES_MS -ForegroundColor Green

# ── Helper ────────────────────────────────────────────────────────────────────
function Invoke-Victory([string[]]$Arguments) {
    $previous = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        if ($diagnosticLog) {
            & $exe @Arguments 2>> $diagnosticLog
        } else {
            & $exe @Arguments
        }
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
