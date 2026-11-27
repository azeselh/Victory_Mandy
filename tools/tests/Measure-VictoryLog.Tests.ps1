$ErrorActionPreference = 'Stop'
$measure = Join-Path $PSScriptRoot '../Measure-VictoryLog.ps1'
$fixture = [IO.Path]::GetTempFileName()
try {
    @'
2026-11-26T12:00:00Z INFO queue: frame presented frame=2048 draws=100 dispatches=100
2026-11-26T12:00:10Z INFO queue: frame presented frame=2648 fps=60 ms=16.7 draws=7800 dispatches=600
2026-11-26T12:00:20Z INFO queue: frame presented frame=2848 fps=20 ms=50 draws=1200000 dispatches=800000
2026-11-26T12:00:25Z INFO queue: frame presented frame=3048 fps=40 ms=25 draws=1200000 dispatches=800000
'@ | Set-Content -LiteralPath $fixture
    $result = & $measure -LogPath $fixture
    if ($result.AcceptedWindows -ne 2 -or $result.ExcludedLowLoadWindows -ne 1) {
        throw 'Menu windows were not rejected independently of FPS.'
    }
    if ([Math]::Abs($result.WeightedFps - (400.0 / 15)) -gt 0.00001) {
        throw 'FPS must be weighted by time; averaging 20 and 40 would incorrectly report 30.'
    }
    if ($result.WorstWindowMeanMs -ne 50 -or $result.WindowsBelowTarget -ne 1) {
        throw 'The slow window was not reported.'
    }
    if ($result.Windows[0].DrawsPerFrame -ne 6000 -or $result.Windows[0].DispatchesPerFrame -ne 4000) {
        throw 'Counts must be normalized using the previous logged frame, including excluded menus.'
    }
    $result = & $measure -LogPath $fixture -AfterFrame 2800
    if ($result.AcceptedWindows -ne 1 -or $result.WeightedFps -ne 40 -or $result.StableTargetVerified) {
        throw 'A window crossing the warm-up boundary must be discarded; averages cannot certify stability.'
    }
    $result = & $measure -LogPath $fixture -MinDrawsPerFrame 10000 -AsJson | ConvertFrom-Json
    if ($result.AcceptedWindows -ne 0 -or $null -ne $result.WeightedFps) {
        throw 'No eligible windows must not be reported as a measured FPS.'
    }
    $ansi = [char]27
    "${ansi}[32m2026-11-26T12:00:00Z INFO queue: frame presented frame=2048${ansi}[0m`n2026-11-26T12:00:10Z INFO queue: frame presented frame=2248 draws=1200000 dispatches=800000`n2026-11-26T12:00:11Z ERROR device removed 0x887A0006" | Set-Content -LiteralPath $fixture
    $result = & $measure -LogPath $fixture
    if ($result.AcceptedWindows -ne 1 -or $result.FailureLines.Count -ne 1) {
        throw 'ANSI escapes or device-loss reporting failed.'
    }
    Write-Output 'Measure-VictoryLog: all checks passed.'
} finally {
    Remove-Item -LiteralPath $fixture
}
