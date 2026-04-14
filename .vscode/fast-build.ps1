param(
    [string]$Solution = "$PSScriptRoot\..\visual studio\suspend.sln",
    [string]$Configuration = "Debug",
    [string]$Platform = "x64",
    [switch]$Force
)

$repoRoot = (Resolve-Path "$PSScriptRoot\..").Path
$outExe = Join-Path $repoRoot "visual studio\x64\$Configuration\suspend.exe"

Write-Host ("Fast-build: checking outputs for configuration={0} platform={1}" -f $Configuration, $Platform)

$exeExists = Test-Path $outExe
if ($exeExists) {
    $exeTime = (Get-Item $outExe).LastWriteTime
} else {
    $exeTime = Get-Date "1/1/1970"
}

$srcFiles = Get-ChildItem -Path $repoRoot -Recurse -Include *.cpp,*.c,*.h,*.hpp,*.rc -File -ErrorAction SilentlyContinue | Where-Object { $_.FullName -notmatch "\\.git\\" }
if ($srcFiles) {
    $latestSrcTime = ($srcFiles | ForEach-Object { $_.LastWriteTime } | Sort-Object -Descending | Select-Object -First 1)
} else {
    $latestSrcTime = Get-Date
}

if ($Force -or -not $exeExists -or $latestSrcTime -gt $exeTime) {
    Write-Host "Build required: invoking msbuild wrapper"
    $msbuildWrapper = Join-Path $PSScriptRoot "msbuild.ps1"
    if (Test-Path $msbuildWrapper) {
        & powershell -NoProfile -ExecutionPolicy Bypass -File $msbuildWrapper $Solution "/p:Configuration=$Configuration" "/p:Platform=$Platform"
        exit $LASTEXITCODE
    } else {
        Write-Error ("msbuild wrapper not found at {0}" -f $msbuildWrapper)
        exit 2
    }
} else {
    Write-Host ("Up to date; skipping build. ({0})" -f $outExe)
    exit 0
}
