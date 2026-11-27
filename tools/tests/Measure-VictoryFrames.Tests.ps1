$ErrorActionPreference = 'Stop'
$measure = Join-Path $PSScriptRoot '../Measure-VictoryFrames.ps1'
$csv = [IO.Path]::GetTempFileName()
try {
    @'
sample,frame,elapsed_ms,interval_ms,dropped
1,100,0,0,0
2,104,20,20,0
3,105,60,40,0
4,106,120,60,0
'@ | Set-Content -LiteralPath $csv
    $r = & $measure -CsvPath $csv -AfterSeconds 0
    if ($r.Intervals -ne 3 -or $r.WeightedFps -ne 25 -or $r.MedianMs -ne 40 -or
        $r.P95Ms -ne 60 -or $r.OverBudget -ne 2 -or $r.Over50Ms -ne 1 -or -not $r.CompleteWindow) {
        throw 'Host samples, time weighting or percentiles are incorrect.'
    }
    $r = & $measure -CsvPath $csv -AfterSeconds 0.03 -AsJson | ConvertFrom-Json
    if ($r.Intervals -ne 1 -or $r.MedianMs -ne 60 -or $r.StableTargetVerified) { throw 'Warmup-crossing interval included.' }
    $r = & $measure -CsvPath $csv -AfterSeconds 1
    if ($r.Intervals -ne 0 -or $null -ne $r.WeightedFps -or $r.CompleteWindow) { throw 'Empty capture accepted.' }
    $r = & $measure -CsvPath $csv -AfterSeconds 0 -UntilSeconds 10
    if ($r.CompleteWindow) { throw 'Truncated capture accepted.' }
    Add-Content -LiteralPath $csv -Value "6,108,200,40,1`n7,109,240,40,1`n8,110,NaN,40,1"
    $r = & $measure -CsvPath $csv -AfterSeconds 0
    if ($r.SequenceGaps -ne 1 -or $r.InvalidRows -ne 1 -or $r.CompleteWindow) { throw 'Missing or invalid samples accepted.' }
    Write-Output 'Measure-VictoryFrames: all checks passed.'
} finally { Remove-Item -LiteralPath $csv }
