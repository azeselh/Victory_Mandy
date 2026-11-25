param(
    [string]$TitleExecutable,
    [string]$Executable = "$PSScriptRoot/../target/release/victory.exe",
    [ValidateRange(1, 32)][int]$Jobs = 4,
    [switch]$SkipWarmup,
    [switch]$WarmOnly
)

$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath("$PSScriptRoot/..")
$exe = (Resolve-Path -LiteralPath $Executable).Path

function Invoke-Victory([string[]]$Arguments) {
    # Windows PowerShell 5 wraps redirected native stderr (including INFO
    # logs) as error records. Only the executable's exit code is failure.
    $previous = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        & $exe @Arguments
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    if ($exitCode -ne 0) { throw "VIctory exited with code $exitCode." }
}

Push-Location $repo
try {
    if (-not $TitleExecutable) {
        $config = Invoke-Victory @('config')
        $gameLine = $config | Where-Object { $_ -match '^game_dir\s*:' } | Select-Object -First 1
        if (-not $gameLine) { throw 'Set game_dir in victory.toml or pass -TitleExecutable.' }
        $gameDir = ($gameLine -replace '^game_dir\s*:\s*', '').Trim()
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
            throw 'Close VIctory before warming shaders and starting another run.'
        }
        # Scan shader archives and binaries, without walking every game archive.
        $gameDir = Split-Path -Parent $title
        $sources = @($title)
        $sources += @(Get-ChildItem -LiteralPath $gameDir -File | Where-Object {
            $_.Extension -eq '.dll' -or $_.Name -match '(?i)shader.*\.(rpf|sgd5)$'
        } | ForEach-Object { $_.FullName })
        Write-Host 'Preparing the shader cache for this GPU. Existing translations are reused.'
        Invoke-Victory (@('translate') + $sources + @('--warm', 'all', '--jobs', "$Jobs"))
    }
    if (-not $WarmOnly) {
        Write-Host 'Starting VIctory. On a first run, let pipeline compilation finish at the menu.'
        Invoke-Victory @('run', $title, '--execute')
    }
} finally {
    Pop-Location
}
