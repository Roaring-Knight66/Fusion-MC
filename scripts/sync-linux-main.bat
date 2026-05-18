@echo off
setlocal

pushd "%~dp0.." >nul

echo Syncing src\main.rs to LINUX\src\main.rs...
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0sync-linux-main.ps1" %*
set EXIT_CODE=%ERRORLEVEL%

popd >nul

if not "%EXIT_CODE%"=="0" (
    echo.
    echo sync-linux-main failed with exit code %EXIT_CODE%.
    pause
    exit /b %EXIT_CODE%
)

echo.
echo Done.
pause
exit /b 0
