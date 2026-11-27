# Start-Victory-AMD.ps1 — Optimised launcher for AMD Radeon RX 7900 XT (RDNA 3) + 32 GB RAM
# Branch: amd-enhanced-v2
#
# USAGE:
#   .\tools\Start-Victory-AMD.ps1 -TitleExecutable "C:\...\game_scarlett_master.exe"
#   .\tools\Start-Victory-AMD.ps1              # uses game_dir from victory.toml
#   .\tools\Start-Victory-AMD.ps1 -SkipWarmup  # skip shader pre-translation
#   .\tools\Start-Victory-AMD.ps1 -Output 2560x1440 -UnlockFps
#   -More @{VICTORY_THIN_THREADS='4'} passes a per-run override.
#   -RunDirectory <new folder outside repo> records logs, configuration,
#   binary hash, hardware and host Present intervals. -RunSeconds 360 stops
#   only that run at the limit; the CSV may omit the final buffered second.
#   Analyse a known gameplay segment with tools/Measure-VictoryFrames.ps1.
#   -SeedDataDirectory <folder> copies a fixed save into RunDirectory/data
#   before launch and records its hashes, keeping the source save unchanged.
#   It overrides VICTORY_DATA_DIR for this run. Shader/driver caches remain
#   shared: repeat the baseline after a candidate to check warm-cache effects.
#   CSV elapsed time starts at the first successful Present; RunSeconds starts
#   at process launch. Allow enough startup time to cover the analysis window.
#
# ENVIRONMENT DEFAULTS (preserves explicit overrides except those disabled by -Safe):
#   Async / Parallel CPU: VICTORY_RELAY=2, VICTORY_THIN=1, VICTORY_JOIN_PARTS=8, VICTORY_UPLOAD_AHEAD=1
#   GPU / Barriers:       VICTORY_CARD_STEPS=2, VICTORY_INDIRECT_BATCH=2, VICTORY_CARD_BUFFERS_MIN_KB=64
#   32 GB RAM Safety:     VICTORY_RESIDENCY=1, VICTORY_SWEEP_PER_HEAP=1
#   Renderer Ports:       VICTORY_PORT_{CONSTANTS,DESCRIPTORS,CBV,GUARD,JOURNAL}=1 (disabled with -Safe)

param(
    [string]$TitleExecutable,
    [string]$Executable = "$PSScriptRoot/../target/release/victory.exe",
    [ValidateRange(1, 32)][int]$Jobs = 8,
    [string]$Output,
    [switch]$UnlockFps,
    [switch]$RayTracing,
    [switch]$Safe,
    [switch]$SkipWarmup,
    [switch]$WarmOnly,
    [switch]$Diagnostics,
    [hashtable]$More = @{},
    [string]$RunDirectory,
    [string]$SeedDataDirectory,
    [ValidateRange(0, 3600)][int]$RunSeconds = 0
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
if ($RunSeconds -gt 0 -and -not $RunDirectory) { throw 'RunSeconds requires RunDirectory to retain the measurement.' }
if ($SeedDataDirectory -and -not $RunDirectory) { throw 'SeedDataDirectory requires RunDirectory.' }
$seedFiles = @()
if ($SeedDataDirectory) {
    $SeedDataDirectory = (Resolve-Path -LiteralPath $SeedDataDirectory).Path
    if (-not (Test-Path -LiteralPath $SeedDataDirectory -PathType Container)) { throw 'SeedDataDirectory must be a directory.' }
    $seedEntries = @(Get-Item -LiteralPath $SeedDataDirectory -Force) + @(Get-ChildItem -LiteralPath $SeedDataDirectory -Recurse -Force)
    if ($seedEntries | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }) {
        throw 'SeedDataDirectory must not contain links or junctions.'
    }
    $seedPrefix = $SeedDataDirectory.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    $resolvedRun = [IO.Path]::GetFullPath($RunDirectory).TrimEnd([IO.Path]::DirectorySeparatorChar)
    if ($resolvedRun.Equals($SeedDataDirectory.TrimEnd([IO.Path]::DirectorySeparatorChar), [StringComparison]::OrdinalIgnoreCase) -or
        $resolvedRun.StartsWith($seedPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'RunDirectory must not be inside SeedDataDirectory.'
    }
}

# Changes belong to this invocation, including failures during setup or launch.
$savedEnvironment = @{}
foreach ($entry in Get-ChildItem Env: | Where-Object { $_.Name -like 'VICTORY_*' }) {
    $savedEnvironment[$entry.Name] = $entry.Value
}
try {
    foreach ($key in $More.Keys) {
        if ($key -cnotmatch '^VICTORY_[A-Z0-9_]+$') { throw "Invalid Victory setting: $key" }
        [Environment]::SetEnvironmentVariable($key, [string]$More[$key], 'Process')
    }

    # ── Core AMD RDNA 3 + Parallel Pipeline ──────────────────────────────────────
    if (-not $env:VICTORY_RELAY)            { $env:VICTORY_RELAY = '2' }
    if (-not $env:VICTORY_WIDE_BACK)        { $env:VICTORY_WIDE_BACK = '1' }
    if (-not $env:VICTORY_CARD_REHASH_RUNS) { $env:VICTORY_CARD_REHASH_RUNS = '1' }
    if (-not $env:VICTORY_JOIN_COPIES)      { $env:VICTORY_JOIN_COPIES = '1' }
    if (-not $env:VICTORY_INDIRECT_WINDOW)  { $env:VICTORY_INDIRECT_WINDOW = '1' }
    if (-not $env:VICTORY_RELAY_COPY)       { $env:VICTORY_RELAY_COPY = '1' }
    if (-not $env:VICTORY_CARD_STEPS)       { $env:VICTORY_CARD_STEPS = '2' }
    if (-not $env:VICTORY_INDIRECT_BATCH)   { $env:VICTORY_INDIRECT_BATCH = '2' }
    if (-not $env:VICTORY_TEXTURE_NO_WAIT)  { $env:VICTORY_TEXTURE_NO_WAIT = '1' }
    if (-not $env:VICTORY_MAP_THREADS)      { $env:VICTORY_MAP_THREADS = '1' }
    if (-not $env:VICTORY_TITLE_PRIORITIES) { $env:VICTORY_TITLE_PRIORITIES = '0' }
    if (-not $env:VICTORY_CARD_AWAKE)       { $env:VICTORY_CARD_AWAKE = '1' }
    if (-not $env:VICTORY_SHOW_AFTER)       { $env:VICTORY_SHOW_AFTER = '1' }
    if (-not $env:VICTORY_AMBIENT)          { $env:VICTORY_AMBIENT = '0.8' }

    # ── 32 GB Host RAM Residency & VRAM Buffer Efficiency (cool_fox) ─────────────
    if (-not $env:VICTORY_RESIDENCY)           { $env:VICTORY_RESIDENCY = '1' }
    if (-not $env:VICTORY_SWEEP_PER_HEAP)      { $env:VICTORY_SWEEP_PER_HEAP = '1' }
    if (-not $env:VICTORY_CARD_BUFFERS_MIN_KB) { $env:VICTORY_CARD_BUFFERS_MIN_KB = '64' }

    if ($Safe) {
        foreach ($key in @('VICTORY_UNDER_WIDE', 'VICTORY_LAND_ECHO', 'VICTORY_THIN',
            'VICTORY_UPLOAD_AHEAD', 'VICTORY_PORT_CONSTANTS', 'VICTORY_PORT_DESCRIPTORS',
            'VICTORY_PORT_CBV', 'VICTORY_PORT_GUARD', 'VICTORY_PORT_JOURNAL')) {
            [Environment]::SetEnvironmentVariable($key, '0', 'Process')
        }
        $env:VICTORY_JOIN_PARTS = '1'
    } else {
        if (-not $env:VICTORY_THIN)         { $env:VICTORY_THIN = '1' }
        if (-not $env:VICTORY_JOIN_PARTS)   { $env:VICTORY_JOIN_PARTS = '8' }
        if (-not $env:VICTORY_UPLOAD_AHEAD) { $env:VICTORY_UPLOAD_AHEAD = '1' }

        # ── Renderer Ports 1, 2, 5, 6, 7 (VIctory-7900XTX) ──────────────────────
        if (-not $env:VICTORY_PORT_CONSTANTS)   { $env:VICTORY_PORT_CONSTANTS = '1' }
        if (-not $env:VICTORY_PORT_DESCRIPTORS) { $env:VICTORY_PORT_DESCRIPTORS = '1' }
        if (-not $env:VICTORY_PORT_CBV)         { $env:VICTORY_PORT_CBV = '1' }
        if (-not $env:VICTORY_PORT_GUARD)       { $env:VICTORY_PORT_GUARD = '1' }
        if (-not $env:VICTORY_PORT_JOURNAL)     { $env:VICTORY_PORT_JOURNAL = '1' }
    }

    if ($RayTracing) { $env:VICTORY_BVH_BUILDER = '1' }
    if ($Output)     { $env:VICTORY_OUTPUT = $Output }
    if ($UnlockFps)  { $env:VICTORY_FRAME_INTERVAL_MS = '16.667' }

    # A recorded run owns a new directory, so neither logs nor timing CSVs are overwritten.
    if ($RunDirectory) {
        $RunDirectory = [IO.Path]::GetFullPath($RunDirectory)
        if ($RunDirectory.Equals($repo, [StringComparison]::OrdinalIgnoreCase) -or
            $RunDirectory.StartsWith($repo + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'RunDirectory must be outside the repository.'
        }
        if (Test-Path -LiteralPath $RunDirectory) { throw 'RunDirectory must be a new directory.' }
        New-Item -ItemType Directory -Path $RunDirectory | Out-Null
        if ($SeedDataDirectory) {
            $runData = Join-Path $RunDirectory 'data'
            Copy-Item -LiteralPath $SeedDataDirectory -Destination $runData -Recurse -Force
            # Hash the actual starting copy, before the child can change it.
            $seedFiles = @(Get-ChildItem -LiteralPath $runData -Recurse -File -Force | Sort-Object FullName | ForEach-Object {
                [ordered]@{
                    Path = $_.FullName.Substring($runData.Length + 1)
                    Bytes = $_.Length
                    Sha256 = (Get-FileHash -LiteralPath $_.FullName).Hash
                }
            })
            $env:VICTORY_DATA_DIR = $runData
        }
        $env:VICTORY_FRAME_TIMES = Join-Path $RunDirectory 'frames.csv'
        if (-not $env:VICTORY_LOG) { $env:VICTORY_LOG = 'info' }
    }

    # Diagnostics mode: collect per-frame timing into %LOCALAPPDATA%\victory\diagnostics\
    if ($Diagnostics -and -not $RunDirectory) {
        if (-not $env:VICTORY_LOG) { $env:VICTORY_LOG = 'info' }
        $run = Join-Path "$env:LOCALAPPDATA/victory/diagnostics" ((Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
        New-Item -ItemType Directory -Path $run | Out-Null
        $diagnosticLog = Join-Path $run 'stderr.log'
        Write-Host '[AMD] Diagnostics log:' $diagnosticLog -ForegroundColor Cyan
    }

    Write-Host '[AMD] Mode              =' $(if ($Safe) { 'safe' } else { 'enhanced (THIN + Ports 1,2,5,6,7)' }) -ForegroundColor Green
    Write-Host '[AMD] VICTORY_RELAY     =' $env:VICTORY_RELAY            -ForegroundColor Green
    Write-Host '[AMD] VICTORY_RESIDENCY =' $env:VICTORY_RESIDENCY        -ForegroundColor Green
    Write-Host '[AMD] VICTORY_THIN      =' $(if ($env:VICTORY_THIN) { $env:VICTORY_THIN } else { '0' }) -ForegroundColor Green
    if ($env:VICTORY_OUTPUT)            { Write-Host '[AMD] VICTORY_OUTPUT    =' $env:VICTORY_OUTPUT -ForegroundColor Green }
    if ($env:VICTORY_FRAME_INTERVAL_MS) { Write-Host '[AMD] FRAME_INTERVAL_MS =' $env:VICTORY_FRAME_INTERVAL_MS -ForegroundColor Green }

    # ── Helper ────────────────────────────────────────────────────────────────────
    function Invoke-Victory([string[]]$Arguments) {
        if ($RunDirectory -and $Arguments[0] -eq 'run') {
            $settings = [ordered]@{}
            foreach ($entry in Get-ChildItem Env: | Where-Object { $_.Name -like 'VICTORY_*' } | Sort-Object Name) {
                $settings[$entry.Name] = $entry.Value
            }
            $configPath = if ($env:VICTORY_CONFIG) { $env:VICTORY_CONFIG } else { Join-Path $repo 'victory.toml' }
            $configHash = if (Test-Path -LiteralPath $configPath) { (Get-FileHash -LiteralPath $configPath).Hash } else { $null }
            if ($configHash) { Copy-Item -LiteralPath $configPath -Destination (Join-Path $RunDirectory 'config.toml') }
            $manifest = [ordered]@{
                StartedUtc = [DateTime]::UtcNow.ToString('o')
                Executable = $exe; ExecutableSha256 = (Get-FileHash -LiteralPath $exe).Hash
                Arguments = $Arguments; Settings = $settings
                ConfigPath = $configPath; ConfigSha256 = $configHash
                SeedDataDirectory = $SeedDataDirectory; InitialDataFiles = $seedFiles
                GitHead = (& git -C $repo rev-parse HEAD); GitStatus = @(& git -C $repo status --porcelain)
                Cpu = @(Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors)
                RamBytes = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
                Gpu = @(Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion)
                DurationLimitSeconds = $RunSeconds
                Measurement = 'Host Present intervals; no automatic identification of menus or gameplay.'
            }
            $manifestPath = Join-Path $RunDirectory 'run.json'
            $manifest | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $manifestPath -Encoding UTF8
            # Arguments here are a fixed command, a resolved Windows filename, and
            # switches. Windows filenames cannot contain a double quote.
            $commandLine = ($Arguments | ForEach-Object { '"' + $_ + '"' }) -join ' '
            $process = $null
            try {
                $process = Start-Process -FilePath $exe -ArgumentList $commandLine -WorkingDirectory $repo -WindowStyle Hidden -PassThru `
                    -RedirectStandardOutput (Join-Path $RunDirectory 'stdout.log') -RedirectStandardError (Join-Path $RunDirectory 'stderr.log')
                # Windows PowerShell must retain the native handle to read ExitCode
                # after a short-lived child exits (otherwise it may return null).
                $null = $process.Handle
                $manifest.ProcessId = $process.Id
                $manifest | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $manifestPath -Encoding UTF8
                $limited = $false
                if ($RunSeconds -gt 0) {
                    if (-not $process.WaitForExit($RunSeconds * 1000)) {
                        $limited = $true
                        Stop-Process -Id $process.Id -ErrorAction SilentlyContinue
                    }
                }
                $process.WaitForExit()
                $manifest.ExitCode = $process.ExitCode
                $manifest.StoppedAtDurationLimit = $limited
                $manifest.CompletedUtc = [DateTime]::UtcNow.ToString('o')
                $manifest | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $manifestPath -Encoding UTF8
                if (-not $limited -and $process.ExitCode -ne 0) { throw "Victory exited with code $($process.ExitCode)." }
            } finally {
                if ($null -ne $process) {
                    if (-not $process.HasExited) { Stop-Process -Id $process.Id -ErrorAction SilentlyContinue }
                    $process.Dispose()
                }
            }
            return
        }
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
            Write-Host '[AMD] Pre-translating shader cache with' $Jobs 'jobs (existing translations are reused).' -ForegroundColor Cyan
            Invoke-Victory (@('shaders') + $sources + @('--jobs', "$Jobs"))
        }

        if (-not $WarmOnly) {
            if (Get-Process victory -ErrorAction SilentlyContinue) { throw 'Close Victory before starting another run.' }
            Write-Host '[AMD] Starting Victory — RX 7900 XT + 32 GB RAM profile (amd-enhanced-v2).' -ForegroundColor Cyan
            Invoke-Victory @('run', $title, '--execute')
        }
    } finally {
        Pop-Location
    }
} finally {
    foreach ($entry in Get-ChildItem Env: | Where-Object { $_.Name -like 'VICTORY_*' }) {
        if (-not $savedEnvironment.ContainsKey($entry.Name)) {
            [Environment]::SetEnvironmentVariable($entry.Name, $null, 'Process')
        }
    }
    foreach ($key in $savedEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($key, $savedEnvironment[$key], 'Process')
    }
}
