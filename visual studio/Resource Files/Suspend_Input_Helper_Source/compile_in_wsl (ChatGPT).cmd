@echo off
setlocal

:: This script compiles a C++ file for Linux using WSL.
:: It assumes the script is in the same directory as the source file.
:: The output binary will be placed one directory above the script's location.

REM --- Configuration ---
SET "SOURCE_FILE=linux_helper.cpp"
SET "OUTPUT_NAME=Suspend_Input_Helper_Linux_Binary"

:: --- Path Conversion ---
:: %~dp0 is a special variable in batch scripts that expands to the
:: drive and path of the currently running script.
:: We use it to create an absolute Windows path first.
SET "WIN_SOURCE_PATH=%~dp0%SOURCE_FILE%"
SET "WIN_OUTPUT_PATH=%~dp0..\%OUTPUT_NAME%"

:: Use the 'wsl wslpath' utility to convert the Windows paths
:: to the format that WSL understands (e.g., /mnt/c/...).
echo Converting paths for WSL...
FOR /F "usebackq tokens=*" %%i IN (`wsl wslpath -u "%WIN_SOURCE_PATH%"`) DO SET "WSL_SOURCE_PATH=%%i"
FOR /F "usebackq tokens=*" %%i IN (`wsl wslpath -u "%WIN_OUTPUT_PATH%"`) DO SET "WSL_OUTPUT_PATH=%%i"

:: --- Compilation ---
echo.
echo Compiling %SOURCE_FILE% for Linux...
wsl g++ "%WSL_SOURCE_PATH%" -o "%WSL_OUTPUT_PATH%"

:: --- Result ---
echo.
IF %ERRORLEVEL% EQU 0 (
    echo Successfully compiled!
    echo Output available at: %WIN_OUTPUT_PATH%
) ELSE (
    echo Compilation failed. Please check for errors above.
)

echo.
pause
endlocal