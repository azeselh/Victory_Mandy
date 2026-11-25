param(
    [string]$Executable = "$PSScriptRoot/../target/release/victory.exe",
    [string]$TitleExecutable,
    [string]$DataSeed = "$env:LOCALAPPDATA/victory/data",
    [string]$CacheDirectory,
    [ValidateSet('Release', 'Debug', 'Diagnostics', 'Unknown')]
    [string]$BuildKind = 'Unknown',
    [ValidateSet('default', 'relay2', 'late-wait3', 'wide-back', 'card1mb', 'indirect-single')]
    [string]$Mode = 'default',
    [ValidateRange(5, 3600)][int]$Seconds = 60,
    [string]$OutputRoot = "$env:TEMP/victory-amd-bench",
    [int]$AttachProcessId = 0,
    [string]$LogPath,
    [switch]$SkipReadyPrompt
)

# One factor per process. No game files, saves, settings or caches are deleted.
# Results and private copies of saves must live outside the repository.
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath("$PSScriptRoot/..").TrimEnd('\', '/')
$root = [IO.Path]::GetFullPath($OutputRoot).TrimEnd('\', '/')
if ($root -eq $repo -or $root.StartsWith($repo + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Benchmark output must be outside the repository.'
}
$run = Join-Path $root ((Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + $Mode + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Path $run | Out-Null
$exe = (Resolve-Path -LiteralPath $Executable).Path
$settings = @{}
$launched = $false
if ($AttachProcessId) {
    $process = Get-Process -Id $AttachProcessId
    $exe = $process.Path
    if (-not $LogPath) { throw '-LogPath is required when attaching.' }
} else {
    if (-not $TitleExecutable) { throw '-TitleExecutable is required when launching.' }
    $title = (Resolve-Path -LiteralPath $TitleExecutable).Path
    if (Get-Process victory -ErrorAction SilentlyContinue) { throw 'Close the other VIctory process before a controlled run.' }
    $data = Join-Path $run 'data'
    if (Test-Path -LiteralPath $DataSeed) {
        Copy-Item -LiteralPath $DataSeed -Destination $data -Recurse
    } else {
        New-Item -ItemType Directory -Path $data | Out-Null
    }
    $savedEnv = @{}
    Get-ChildItem Env:VICTORY_* | ForEach-Object { $savedEnv[$_.Name] = $_.Value }
    try {
        # Preserve path configuration; remove all inherited tuning switches.
        foreach ($key in $savedEnv.Keys) {
            if ($key -notin @('VICTORY_CONFIG', 'VICTORY_GAME_DIR', 'VICTORY_CACHE_DIR')) {
                [Environment]::SetEnvironmentVariable($key, $null, 'Process')
            }
        }
        $env:VICTORY_DATA_DIR = $data
        $env:VICTORY_LOG = 'info'
        if ($CacheDirectory) { $env:VICTORY_CACHE_DIR = [IO.Path]::GetFullPath($CacheDirectory) }
        switch ($Mode) {
            'relay2' { $env:VICTORY_RELAY = '2' }
            'late-wait3' { $env:VICTORY_LATE_WAIT = '3' }
            'wide-back' { $env:VICTORY_WIDE_BACK = '1' }
            'card1mb' { $env:VICTORY_CARD_BUFFERS_MIN_MB = '1' }
            'indirect-single' { $env:VICTORY_INDIRECT_SINGLE = '1' }
        }
        Get-ChildItem Env:VICTORY_* | ForEach-Object { $settings[$_.Name] = $_.Value }
        $LogPath = Join-Path $run 'stderr.log'
        $process = Start-Process -FilePath $exe -ArgumentList @('run', ('"' + $title + '"'), '--execute') `
            -WorkingDirectory $repo -WindowStyle Hidden -PassThru `
            -RedirectStandardOutput (Join-Path $run 'stdout.log') -RedirectStandardError $LogPath
        $launched = $true
    } finally {
        Get-ChildItem Env:VICTORY_* | ForEach-Object { [Environment]::SetEnvironmentVariable($_.Name, $null, 'Process') }
        foreach ($key in $savedEnv.Keys) { [Environment]::SetEnvironmentVariable($key, $savedEnv[$key], 'Process') }
    }
}
$confirmed = $false
if (-not $SkipReadyPrompt) {
    Write-Host 'Load the same save, keep the camera still, and wait for compilation/streaming to settle.'
    Write-Host 'Do not run builds, tests, captures or another game during measurement.'
    $answer = Read-Host 'Type READY when the scene is ready (anything else cancels measurement)'
    if ($answer -ne 'READY') { throw "Measurement cancelled. Process $($process.Id) is still available; close it normally." }
    $confirmed = $true
}
$start = [DateTime]::UtcNow
$process.Refresh()
$lastCpu = $process.TotalProcessorTime.TotalMilliseconds
$lastTime = [Diagnostics.Stopwatch]::StartNew()
$cpuCount = [Environment]::ProcessorCount
$samples = [Collections.Generic.List[object]]::new()
$counterErrors = [Collections.Generic.HashSet[string]]::new()
$measurement = [Diagnostics.Stopwatch]::StartNew()
while ($measurement.Elapsed.TotalSeconds -lt $Seconds) {
    Start-Sleep -Milliseconds 1000
    $process.Refresh()
    if ($process.HasExited) { break }
    $cpuNow = $process.TotalProcessorTime.TotalMilliseconds
    $elapsedMs = $lastTime.Elapsed.TotalMilliseconds
    $lastTime.Restart()
    $cpuPct = 100 * ($cpuNow - $lastCpu) / $elapsedMs / $cpuCount
    $lastCpu = $cpuNow
    $gpuMax = $null
    $dedicated = $null
    $shared = $null
    try {
        $engines = @(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine -Filter "Name LIKE 'pid_$($process.Id)_%'")
        if ($engines.Count) { $gpuMax = ($engines | Measure-Object UtilizationPercentage -Maximum).Maximum }
        $memory = @(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUProcessMemory -Filter "Name LIKE 'pid_$($process.Id)_%'")
        if ($memory.Count) {
            $dedicated = ($memory | Measure-Object DedicatedUsage -Sum).Sum
            $shared = ($memory | Measure-Object SharedUsage -Sum).Sum
        }
    } catch { [void]$counterErrors.Add($_.Exception.Message) }
    $samples.Add([pscustomobject]@{
        Utc = [DateTime]::UtcNow.ToString('o')
        CpuPercent = $cpuPct
        WorkingSetBytes = $process.WorkingSet64
        PrivateBytes = $process.PrivateMemorySize64
        GpuEngineMaxPercent = $gpuMax
        DedicatedBytes = $dedicated
        SharedBytes = $shared
        Caption = $process.MainWindowTitle
    })
}
$end = [DateTime]::UtcNow
$samples | Export-Csv -LiteralPath (Join-Path $run 'process.csv') -NoTypeInformation
$frames = [Collections.Generic.List[object]]::new()
$pattern = '([\d.]+) fps, ([\d.]+) ms/frame \(([\d.]+) preparing: copies ([\d.]+) ms [\d.]+ MB, textures ([\d.]+) ms [\d.]+ MB; ([\d.]+) on the GPU; ([\d.]+) copying back [\d.]+ MB\), ([\d.]+) draws/frame, dropped ([\d.]+).*?and ([\d.]+) indirect;'
$culture = [Globalization.CultureInfo]::InvariantCulture
Get-Content -LiteralPath $LogPath | ForEach-Object {
    $line = $_ -replace '\x1b\[[0-9;]*m', ''
    if ($line -match '^(\S+) .*' -and $line -match $pattern) {
        $timestamp = [DateTime]::Parse(($line -split ' ')[0], $culture).ToUniversalTime()
        # Existing log metrics cover about one second; discard the first
        # boundary window rather than including pre-measurement work.
        if ($timestamp -ge $start.AddSeconds(2) -and $timestamp -le $end) {
            $values = 1..10 | ForEach-Object { [double]::Parse($Matches[$_], $culture) }
            $frames.Add([pscustomobject]@{
                Utc = $timestamp.ToString('o'); Fps = $values[0]; WindowMeanFrameMs = $values[1]
                PrepareMs = $values[2]; CopyMs = $values[3]; TextureMs = $values[4]
                CpuWaitForGpuMs = $values[5]; CopyBackMs = $values[6]
                Draws = $values[7]; DroppedDraws = $values[8]; IndirectCounter = $values[9]
            })
        }
    }
}
$frames | Export-Csv -LiteralPath (Join-Path $run 'frames.csv') -NoTypeInformation
[ordered]@{
    Tool = 'VICTORY AMD ENHANCED benchmark'; Mode = $Mode; Executable = $exe; BuildKind = $BuildKind
    Sha256 = (Get-FileHash -LiteralPath $exe).Hash
    ProcessId = $process.Id; StartedUtc = $start.ToString('o'); EndedUtc = $end.ToString('o')
    SceneConfirmed = $confirmed; Launched = $launched; Environment = $settings
    LogPath = $LogPath; FrameWindows = $frames.Count; ProcessSamples = $samples.Count
    CounterErrors = @($counterErrors)
    Notes = @('Build type must be verified from its build command; file paths alone do not prove Release.',
        'CpuWaitForGpuMs is CPU waiting, NOT GPU execution time.',
        'Frame rows are sampled logging windows, not individual frame times; no 1% lows or frame p99 can be inferred.',
        'GpuEngineMaxPercent is the busiest process GPU engine, not total adapter utilization.',
        'The existing IndirectCounter does not include every indirect mesh/dispatch path.',
        'Shader cache keys include the executable: warm up each binary separately.',
        'Close the game normally. This tool leaves the process and private data available.')
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $run 'metadata.json') -Encoding UTF8
Write-Host "Results: $run"
Write-Host "Sampled log windows: $($frames.Count); process samples: $($samples.Count). Close the game normally."
