# Summarize queue timing windows without mistaking menu FPS for gameplay.
# These are window averages: they cannot establish individual-frame p99 or stable 30 FPS.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$LogPath,
    [ValidateRange(0, 1000000)][double]$MinDrawsPerFrame = 1000,
    [ValidateRange(0, 1000000)][double]$MinDispatchesPerFrame = 100,
    [ValidateRange(0, 2147483647)][long]$AfterFrame = 2048,
    [ValidateRange(1, 1000)][double]$TargetFps = 30,
    [switch]$AsJson
)

$ErrorActionPreference = 'Stop'
$culture = [Globalization.CultureInfo]::InvariantCulture
$windows = [Collections.Generic.List[object]]::new()
$previousFrame = $null
$previousTime = $null
$lowLoad = 0
$invalid = 0
$failureLines = [Collections.Generic.List[string]]::new()
$resolvedLog = (Resolve-Path -LiteralPath $LogPath).Path

foreach ($raw in [IO.File]::ReadLines($resolvedLog)) {
    $line = $raw -replace '\x1b\[[0-9;]*m', ''
    if ($line -match '(?i)0x887a000[567]|panicked at|memory allocation .* failed|out of memory|device (removed|lost)') {
        if ($failureLines.Count -lt 10) { $failureLines.Add($line) }
    }
    if ($line -notmatch 'frame presented.*\bframe=(\d+)') { continue }
    $frame = [long]$Matches[1]
    $timestamp = [DateTimeOffset]::MinValue
    $validTime = [DateTimeOffset]::TryParse(($line -split '\s+')[0], $culture,
        [Globalization.DateTimeStyles]::None, [ref]$timestamp)
    $firstFrame = $previousFrame
    $firstTime = $previousTime
    $previousFrame = $frame
    $previousTime = if ($validTime) { $timestamp } else { $null }
    if ($null -eq $firstFrame -or $null -eq $firstTime -or -not $validTime) { continue }
    $count = $frame - $firstFrame
    $seconds = ($timestamp - $firstTime).TotalSeconds
    if ($count -le 0 -or $seconds -le 0) { $invalid++; continue }
    # Discard a whole window that crosses the warm-up boundary.
    if ($firstFrame -lt $AfterFrame) { continue }
    if ($line -notmatch '\bdraws=(\d+)') { $invalid++; continue }
    $draws = [double]::Parse($Matches[1], $culture) / $count
    if ($line -notmatch '\bdispatches=(\d+)') { $invalid++; continue }
    $dispatches = [double]::Parse($Matches[1], $culture) / $count
    if ($draws -lt $MinDrawsPerFrame -or $dispatches -lt $MinDispatchesPerFrame) {
        $lowLoad++
        continue
    }
    $windows.Add([pscustomobject]@{
        FromFrame = $firstFrame; ToFrame = $frame; Frames = $count; Seconds = $seconds
        Fps = $count / $seconds; MeanFrameMs = 1000 * $seconds / $count
        DrawsPerFrame = $draws; DispatchesPerFrame = $dispatches
    })
}

$frames = 0L
$seconds = 0.0
foreach ($window in $windows) { $frames += $window.Frames; $seconds += $window.Seconds }
$fps = if ($seconds -gt 0) { $frames / $seconds } else { $null }
$worstMs = if ($windows.Count) { ($windows | Measure-Object MeanFrameMs -Maximum).Maximum } else { $null }
$belowTarget = @($windows | Where-Object { $_.Fps -lt $TargetFps }).Count
$result = [pscustomobject][ordered]@{
    LogPath = $resolvedLog
    AfterFrame = $AfterFrame
    MinDrawsPerFrame = $MinDrawsPerFrame
    MinDispatchesPerFrame = $MinDispatchesPerFrame
    AcceptedWindows = $windows.Count
    ExcludedLowLoadWindows = $lowLoad
    InvalidWindows = $invalid
    Frames = $frames
    MeasuredSeconds = $seconds
    WeightedFps = $fps
    WorstWindowMeanMs = $worstMs
    TargetFps = $TargetFps
    WindowsBelowTarget = $belowTarget
    FailureLines = @($failureLines.ToArray())
    StableTargetVerified = $false
    Notes = @(
        'Draw/dispatch counts are totals since the previous logged frame; normalized by that interval.',
        'Load thresholds reject menus but do not prove that gameplay, camera, save or scene match.',
        'FPS is derived from timestamp differences, weighted by frame count, not averaged across windows.',
        'Window means cannot prove stable FPS, individual-frame percentiles, visual correctness or absence of crashes.'
    )
    Windows = @($windows.ToArray())
}
if ($AsJson) { $result | ConvertTo-Json -Depth 5 } else { $result }
