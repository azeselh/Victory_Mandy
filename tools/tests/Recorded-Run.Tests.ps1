# Exercise process quoting, provenance and termination without launching a game.
$ErrorActionPreference = 'Stop'
$launcher = Join-Path $PSScriptRoot '../Start-Victory-AMD.ps1'
$prefix = Join-Path ([IO.Path]::GetTempPath()) ('victory recorded test ' + [guid]::NewGuid().ToString('N'))
$mockExe = $prefix + '.exe'
$run = $prefix + ' run'
$limitedRun = $prefix + ' limited'
$seed = $prefix + ' seed'
try {
    New-Item -ItemType Directory -Path (Join-Path $seed 'saves') | Out-Null
    $seedFile = Join-Path $seed 'saves/progress.txt'
    Set-Content -LiteralPath $seedFile -Value 'starting save'
    $seedHash = (Get-FileHash -LiteralPath $seedFile).Hash
    Add-Type -OutputAssembly $mockExe -OutputType ConsoleApplication -TypeDefinition @'
using System;
class RecordedRunTest {
    static void Main(string[] args) {
        Console.WriteLine(Environment.GetEnvironmentVariable("VICTORY_THIN"));
        Console.WriteLine(args[1]);
        if (Environment.GetEnvironmentVariable("VICTORY_TEST_MUTATE_SAVE") == "1") {
            System.IO.File.WriteAllText(System.IO.Path.Combine(Environment.GetEnvironmentVariable("VICTORY_DATA_DIR"), "saves", "progress.txt"), "changed by child");
        }
        if (Environment.GetEnvironmentVariable("VICTORY_TEST_SLEEP") == "1") System.Threading.Thread.Sleep(10000);
    }
}
'@
    & $launcher -Executable $mockExe -TitleExecutable $mockExe -SkipWarmup -RunDirectory $run -Safe `
        -SeedDataDirectory $seed -More @{VICTORY_TEST_MUTATE_SAVE='1'}
    $m = Get-Content -LiteralPath (Join-Path $run 'run.json') -Raw | ConvertFrom-Json
    $stdout = Get-Content -LiteralPath (Join-Path $run 'stdout.log')
    if ($m.ExitCode -ne 0 -or $m.StoppedAtDurationLimit -or $m.ExecutableSha256 -ne (Get-FileHash $mockExe).Hash -or
        $stdout[0] -ne '0' -or $stdout[1] -ne $mockExe) { throw 'Recorded invocation or provenance incorrect.' }
    if ($m.SeedDataDirectory -ne $seed -or $m.InitialDataFiles.Count -ne 1 -or
        $m.InitialDataFiles[0].Sha256 -ne $seedHash -or
        $m.InitialDataFiles[0].Path -ne 'saves\progress.txt' -or
        (Get-FileHash -LiteralPath $seedFile).Hash -ne $seedHash -or
        (Get-Content -LiteralPath (Join-Path $run 'data/saves/progress.txt') -Raw) -ne 'changed by child') {
        throw 'Seed save was not isolated or its initial hash was not recorded.'
    }
    foreach ($invalidRun in @('', (Join-Path $seed 'nested-run'))) {
        $rejected = $false
        try {
            & $launcher -Executable $mockExe -TitleExecutable $mockExe -SkipWarmup -SeedDataDirectory $seed -RunDirectory $invalidRun
        } catch { $rejected = $_.Exception.Message -match 'requires RunDirectory|inside SeedDataDirectory' }
        if (-not $rejected) { throw 'Unsafe seed/run directory combination accepted.' }
    }
    & $launcher -Executable $mockExe -TitleExecutable $mockExe -SkipWarmup -RunDirectory $limitedRun -RunSeconds 1 -More @{VICTORY_TEST_SLEEP='1'}
    $m = Get-Content -LiteralPath (Join-Path $limitedRun 'run.json') -Raw | ConvertFrom-Json
    if (-not $m.StoppedAtDurationLimit -or (Get-Process -Id $m.ProcessId -ErrorAction SilentlyContinue)) {
        throw 'The benchmark did not stop its own process.'
    }
    Write-Output 'Recorded run: all checks passed.'
} finally {
    # Only explicitly named test files; no recursive removal of computed paths.
    foreach ($directory in @($run, $limitedRun)) {
        foreach ($relative in @('data/saves/progress.txt', 'data/saves', 'data')) {
            $cleanupPath = Join-Path $directory $relative
            if (Test-Path -LiteralPath $cleanupPath) { Remove-Item -LiteralPath $cleanupPath }
        }
        foreach ($name in @('run.json','stdout.log','stderr.log','config.toml')) {
            Remove-Item -LiteralPath (Join-Path $directory $name) -ErrorAction SilentlyContinue
        }
        if (Test-Path -LiteralPath $directory) { Remove-Item -LiteralPath $directory }
    }
    Remove-Item -LiteralPath $mockExe -ErrorAction SilentlyContinue
    foreach ($relative in @('saves/progress.txt', 'saves', '')) {
        $cleanupPath = if ($relative) { Join-Path $seed $relative } else { $seed }
        if (Test-Path -LiteralPath $cleanupPath) { Remove-Item -LiteralPath $cleanupPath }
    }
}
