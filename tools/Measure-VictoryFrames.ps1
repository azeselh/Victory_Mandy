# Host DXGI Present intervals, not GPU timings or display scanout.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$CsvPath,
    [ValidateRange(0, 86400)][double]$AfterSeconds = 180,
    [ValidateRange(0, 86400)][double]$UntilSeconds = 0,
    [ValidateRange(1, 1000)][double]$TargetFps = 30,
    [switch]$AsJson
)
$ErrorActionPreference = 'Stop'
if ($UntilSeconds -gt 0 -and $UntilSeconds -le $AfterSeconds) { throw 'UntilSeconds must exceed AfterSeconds.' }
$culture = [Globalization.CultureInfo]::InvariantCulture
$samples = [Collections.Generic.List[double]]::new()
$previous = $null
$gaps = 0
$invalid = 0
$dropped = 0L
$first = $null
$last = $null
foreach ($row in Import-Csv -LiteralPath $CsvPath) {
    $sequence = 0L; $lost = 0L; $elapsed = 0.0; $interval = 0.0
    if (-not [long]::TryParse($row.sample, [ref]$sequence) -or
        -not [long]::TryParse($row.dropped, [ref]$lost) -or
        -not [double]::TryParse($row.elapsed_ms, [Globalization.NumberStyles]::Float, $culture, [ref]$elapsed) -or
        -not [double]::TryParse($row.interval_ms, [Globalization.NumberStyles]::Float, $culture, [ref]$interval) -or
        [double]::IsNaN($elapsed) -or [double]::IsInfinity($elapsed) -or
        [double]::IsNaN($interval) -or [double]::IsInfinity($interval) -or
        $sequence -lt 1 -or $lost -lt 0 -or $elapsed -lt 0 -or $interval -lt 0) {
        $invalid++; $previous = $null; continue
    }
    $inWindow = $elapsed -ge 1000 * $AfterSeconds -and ($UntilSeconds -eq 0 -or $elapsed -le 1000 * $UntilSeconds)
    if ($inWindow) { $dropped = [math]::Max($dropped, $lost) }
    if ($null -ne $previous) {
        if ($sequence -ne $previous.Sequence + 1 -or $elapsed -le $previous.Elapsed) {
            if ($inWindow) { $gaps++ }
        } elseif ([math]::Abs(($elapsed - $previous.Elapsed) - $interval) -gt 0.01) {
            if ($inWindow) { $invalid++ }
        } elseif ($inWindow -and $previous.Elapsed -ge 1000 * $AfterSeconds -and $interval -gt 0) {
            $samples.Add($interval)
            if ($null -eq $first) { $first = $previous.Elapsed / 1000 }
            $last = $elapsed / 1000
        }
    }
    $previous = @{Sequence=$sequence; Elapsed=$elapsed}
}
$sorted = @($samples | Sort-Object)
function Percentile([double]$fraction) {
    if ($sorted.Count -eq 0) { return $null }
    $sorted[[math]::Max(0, [int][math]::Ceiling($fraction * $sorted.Count) - 1)]
}
$totalMs = ($samples | Measure-Object -Sum).Sum
$fps = if ($samples.Count -gt 0) { 1000 * $samples.Count / $totalMs } else { $null }
$budget = 1000 / $TargetFps
$result = [pscustomobject][ordered]@{
    CsvPath = (Resolve-Path -LiteralPath $CsvPath).Path
    Measurement = 'Intervals between successful host DXGI Present calls; not display scanout.'
    AfterSeconds = $AfterSeconds; UntilSeconds = $UntilSeconds
    FirstIntervalStartSeconds = $first; LastPresentSeconds = $last
    Intervals = $samples.Count; MeasuredSeconds = $totalMs / 1000
    WeightedFps = $fps; MedianMs = Percentile 0.5; P95Ms = Percentile 0.95; P99Ms = Percentile 0.99
    MaxMs = if ($sorted.Count) { $sorted[-1] } else { $null }
    TargetFps = $TargetFps
    OverBudget = @($samples | Where-Object { $_ -gt $budget + 0.001 }).Count
    Over50Ms = @($samples | Where-Object { $_ -gt 50 }).Count
    Over100Ms = @($samples | Where-Object { $_ -gt 100 }).Count
    SequenceGaps = $gaps; InvalidRows = $invalid; ReportedDroppedSamples = $dropped
    CompleteWindow = ($samples.Count -gt 0 -and $gaps -eq 0 -and $invalid -eq 0 -and $dropped -eq 0 -and
        $first -le $AfterSeconds + 1 -and ($UntilSeconds -eq 0 -or $last -ge $UntilSeconds - 1))
    StableTargetVerified = $false
    Notes = 'Select the same gameplay segment. Warmup, menus, occlusion and a truncated tail must be reviewed separately; this file cannot certify visual correctness or long-run stability.'
}
if ($AsJson) { $result | ConvertTo-Json -Depth 4 } else { $result }
