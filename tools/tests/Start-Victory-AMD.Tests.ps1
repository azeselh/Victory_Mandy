$ErrorActionPreference = 'Stop'
$launcher = Join-Path $PSScriptRoot '../Start-Victory-AMD.ps1'
$mock = Join-Path ([IO.Path]::GetTempPath()) ('victory-launch-test-' + [guid]::NewGuid().ToString('N') + '.ps1')
$saved = @{}
foreach ($entry in Get-ChildItem Env: | Where-Object { $_.Name -like 'VICTORY_*' }) {
    $saved[$entry.Name] = $entry.Value
}
function Snapshot {
    $values = @{}
    foreach ($entry in Get-ChildItem Env: | Where-Object { $_.Name -like 'VICTORY_*' }) {
        $values[$entry.Name] = $entry.Value
    }
    $values
}
function Assert-Restored($expected) {
    $actual = Snapshot
    if ($actual.Count -ne $expected.Count) { throw 'Environment entry count changed.' }
    foreach ($key in $expected.Keys) {
        if ($actual[$key] -cne $expected[$key]) { throw "Environment changed: $key" }
    }
}
try {
    @'
$values = @{}
foreach ($entry in Get-ChildItem Env: | Where-Object { $_.Name -like 'VICTORY_*' }) {
    $values[$entry.Name] = $entry.Value
}
$values | ConvertTo-Json -Compress
$global:LASTEXITCODE = if ($env:VICTORY_TEST_FAIL -eq '1') { 7 } else { 0 }
'@ | Set-Content -LiteralPath $mock
    foreach ($entry in Get-ChildItem Env: | Where-Object { $_.Name -like 'VICTORY_*' }) {
        [Environment]::SetEnvironmentVariable($entry.Name, $null, 'Process')
    }
    $env:VICTORY_THIN = '1'
    $env:VICTORY_PORT_JOURNAL = '1'
    $env:VICTORY_UNDER_WIDE = '1'
    $env:VICTORY_OUTPUT = '3840x2160'
    $before = Snapshot
    $argsForLaunch = @{ Executable=$mock; TitleExecutable=$mock; SkipWarmup=$true }
    $normal = & $launcher @argsForLaunch -Output 1920x1080 -More @{VICTORY_THIN_THREADS='4'} | ConvertFrom-Json
    if ($normal.VICTORY_THIN -ne '1' -or $normal.VICTORY_THIN_THREADS -ne '4' -or $normal.VICTORY_OUTPUT -ne '1920x1080') {
        throw 'Explicit settings did not reach the child.'
    }
    Assert-Restored $before
    $safe = & $launcher @argsForLaunch -Safe -More @{VICTORY_PORT_CBV='1'} | ConvertFrom-Json
    foreach ($key in @('VICTORY_THIN','VICTORY_UNDER_WIDE','VICTORY_LAND_ECHO','VICTORY_UPLOAD_AHEAD',
        'VICTORY_PORT_CONSTANTS','VICTORY_PORT_DESCRIPTORS','VICTORY_PORT_CBV','VICTORY_PORT_GUARD','VICTORY_PORT_JOURNAL')) {
        if ($safe.$key -ne '0') { throw "Safe mode left $key enabled." }
    }
    if ($safe.VICTORY_JOIN_PARTS -ne '1') { throw 'Safe mode still joins submissions.' }
    Assert-Restored $before
    $normalAgain = & $launcher @argsForLaunch | ConvertFrom-Json
    if ($normalAgain.VICTORY_PORT_CBV -ne '1') { throw 'Safe mode contaminated the next invocation.' }
    Assert-Restored $before
    $failed = $false
    try { & $launcher @argsForLaunch -More @{VICTORY_TEST_FAIL='1'} | Out-Null } catch {
        $failed = $_.Exception.Message -match 'code 7'
    }
    if (-not $failed) { throw 'Child failure was not reported.' }
    Assert-Restored $before
    $missing = $false
    try { & $launcher -Executable $mock -TitleExecutable ($mock + '.missing') -SkipWarmup | Out-Null } catch { $missing = $true }
    if (-not $missing) { throw 'Missing title was not rejected.' }
    Assert-Restored $before
    Write-Output 'Start-Victory-AMD: all checks passed.'
} finally {
    foreach ($entry in Get-ChildItem Env: | Where-Object { $_.Name -like 'VICTORY_*' }) {
        [Environment]::SetEnvironmentVariable($entry.Name, $null, 'Process')
    }
    foreach ($key in $saved.Keys) { [Environment]::SetEnvironmentVariable($key, $saved[$key], 'Process') }
    Remove-Item -LiteralPath $mock -ErrorAction SilentlyContinue
}
