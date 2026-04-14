$locations = @(
    'C:\Program Files\Microsoft Visual Studio\18\Insiders',
    'C:\Program Files\Microsoft Visual Studio\18\Insiders\MSBuild\Current\Bin',
    'C:\Program Files\dotnet',
    'C:\Program Files\dotnet\shared'
)
foreach($loc in $locations){
    if(Test-Path $loc){
        Write-Host "--- Searching: $loc ---"
        Get-ChildItem -Path $loc -Filter 'System.Text.Json.dll' -Recurse -ErrorAction SilentlyContinue | ForEach-Object {
            $f = $_.FullName
            Write-Host $f
            try { $v = (Get-Item $f).VersionInfo.FileVersion; Write-Host "  FileVersion: $v" } catch { Write-Host "  FileVersion: <error>" }
        }
    } else {
        Write-Host "(not found) $loc"
    }
}
# Also search dotnet SDK subfolders for System.Text.Json
$sdkRoot = 'C:\Program Files\dotnet\sdk'
if(Test-Path $sdkRoot){
    Write-Host "--- Searching SDKs under $sdkRoot ---"
    Get-ChildItem -Path $sdkRoot -Directory -ErrorAction SilentlyContinue | ForEach-Object {
        $s = $_.FullName
        Get-ChildItem -Path $s -Filter 'System.Text.Json.dll' -Recurse -ErrorAction SilentlyContinue | ForEach-Object {
            $f = $_.FullName
            Write-Host $f
            try { $v = (Get-Item $f).VersionInfo.FileVersion; Write-Host "  FileVersion: $v" } catch { Write-Host "  FileVersion: <error>" }
        }
    }
}
# Also list any System.Text.Json.* files in MSBuild bin
$msbuildBin = 'C:\Program Files\Microsoft Visual Studio\18\Insiders\MSBuild\Current\Bin'
if(Test-Path $msbuildBin){
    Write-Host "--- MSBuild bin contents matching System.Text.Json* ---"
    Get-ChildItem -Path $msbuildBin -Filter 'System.Text.Json*' -Recurse -ErrorAction SilentlyContinue | ForEach-Object {
        Write-Host $_.FullName
        try { Write-Host "  FileVersion: " (Get-Item $_.FullName).VersionInfo.FileVersion } catch { Write-Host "  FileVersion: <error>" }
    }
}
Write-Host "Done."