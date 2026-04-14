param(
    [Parameter(Mandatory=$true)]
    [string]$Solution,
    [Parameter(ValueFromRemainingArguments=$true)]
    [string[]]$MsbuildArgs
)

function Find-VsWhere {
    $candidates = @(
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\Installer\vswhere.exe"
    )
    foreach ($p in $candidates) { if (Test-Path $p) { return $p } }
    try {
        $where = & where.exe vswhere 2>$null
        if ($where) { return ($where -split "\r?\n")[0] }
    } catch { }
    return $null
}

$msbuildExe = $null

# 1) Try vswhere (installed location or on PATH)
$vswhere = Find-VsWhere
if ($vswhere) {
    try { 
        $installPath = & $vswhere -latest -products * -requires Microsoft.Component.MSBuild -property installationPath 2>$null
        if ($installPath) {
            $candidates = @(
                Join-Path $installPath 'MSBuild\Current\Bin\MSBuild.exe',
                Join-Path $installPath 'MSBuild\15.0\Bin\MSBuild.exe',
                Join-Path $installPath 'MSBuild\14.0\Bin\MSBuild.exe'
            )
            foreach ($mp in $candidates) { if (Test-Path $mp) { $msbuildExe = $mp; break } }
        }
    } catch { }
}

# 2) Try msbuild on PATH
if (-not $msbuildExe) {
    try {
        $whereOut = & where.exe msbuild 2>$null
        if ($whereOut) { $msbuildExe = $whereOut -split "\r?\n" | Select-Object -First 1 }
    } catch { }
}

# 3) Scan common install folders under Program Files / Program Files (x86)
if (-not $msbuildExe) {
    $roots = @()
    if ($env:ProgramFiles) { $roots += Join-Path ${env:ProgramFiles} 'Microsoft Visual Studio' }
    if (${env:ProgramFiles(x86)}) { $roots += Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio' }
    foreach ($root in $roots) {
        if (-not (Test-Path $root)) { continue }
        try {
            Get-ChildItem -Directory -Path (Join-Path $root '*') -ErrorAction SilentlyContinue | ForEach-Object {
                $inst = $_.FullName
                $candidates = @(
                    Join-Path $inst 'MSBuild\Current\Bin\MSBuild.exe',
                    Join-Path $inst 'MSBuild\15.0\Bin\MSBuild.exe',
                    Join-Path $inst 'MSBuild\14.0\Bin\MSBuild.exe',
                    Join-Path $inst 'BuildTools\MSBuild\Current\Bin\MSBuild.exe'
                )
                foreach ($mp in $candidates) { if (Test-Path $mp) { $msbuildExe = $mp; break } }
                if ($msbuildExe) { return }
            }
        } catch { }
        if ($msbuildExe) { break }
    }
}

# 4) Deep scan: try to find vswhere.exe or MSBuild.exe recursively under Visual Studio folders
if (-not $msbuildExe) {
    foreach ($root in $roots) {
        if (-not (Test-Path $root)) { continue }
        try {
            $foundVswhere = Get-ChildItem -Path $root -Recurse -Filter vswhere.exe -ErrorAction SilentlyContinue -Force | Select-Object -First 1
            if ($foundVswhere) {
                $vswhere = $foundVswhere.FullName
                Write-Host "Found vswhere at: $vswhere"
                try {
                    $installPath = & $vswhere -latest -products * -requires Microsoft.Component.MSBuild -property installationPath 2>$null
                    if ($installPath) {
                        $candidate = Join-Path $installPath 'MSBuild\Current\Bin\MSBuild.exe'
                        if (Test-Path $candidate) { $msbuildExe = $candidate; break }
                    }
                } catch { }
            }

            $foundMsbuild = Get-ChildItem -Path $root -Recurse -Filter MSBuild.exe -ErrorAction SilentlyContinue -Force | Select-Object -First 1
            if ($foundMsbuild) {
                $msbuildExe = $foundMsbuild.FullName
                Write-Host "Found MSBuild.exe at: $msbuildExe"
                break
            }
        } catch { }
        if ($msbuildExe) { break }
    }
}

if (-not $msbuildExe) {
    Write-Error "MSBuild not found. Install Visual Studio Build Tools or open the Developer Command Prompt (VsDevCmd.bat)."
    exit 1
}

$argsList = @()
$argsList += $Solution
if ($MsbuildArgs) { $argsList += $MsbuildArgs }

## Quote arguments that contain spaces so MSBuild receives each arg intact
$quotedArgs = $argsList | ForEach-Object { if ($_ -match '\s') { '"' + $_ + '"' } else { $_ } }
$argString = $quotedArgs -join ' '
Write-Host "Running: $msbuildExe $argString"
## Try to find VsDevCmd.bat to setup the VC environment so the correct cl.exe/toolset is used.
$vsDevCmd = $null
try {
    $found = Get-ChildItem -Path (Split-Path $msbuildExe -Parent) -Recurse -Filter VsDevCmd.bat -ErrorAction SilentlyContinue -Force | Select-Object -First 1
    if ($found) { $vsDevCmd = $found.FullName }
} catch { }

if (-not $vsDevCmd) {
    # also search the Visual Studio install roots if available
    foreach ($root in $roots) {
        try {
            $found = Get-ChildItem -Path $root -Recurse -Filter VsDevCmd.bat -ErrorAction SilentlyContinue -Force | Select-Object -First 1
            if ($found) { $vsDevCmd = $found.FullName; break }
        } catch { }
    }
}

if ($vsDevCmd) {
    Write-Host "Using VsDevCmd: $vsDevCmd"
    $cmd = "call `"$vsDevCmd`" && `"$msbuildExe`" $argString"
    & cmd.exe /d /s /c $cmd
    $exitCode = $LASTEXITCODE
} else {
    & $msbuildExe @argsList
    $exitCode = $LASTEXITCODE
}

exit $exitCode
