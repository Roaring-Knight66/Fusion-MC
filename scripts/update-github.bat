@echo off
setlocal

pushd "%~dp0.." >nul

if "%~1"=="" (
    echo.
    echo Fusion Launcher GitHub updater
    echo.
    echo If this is your first run, you will be asked for your SSH remote:
    echo   git@github.com:USER/REPO.git
    echo.
    echo You can also pass it directly:
    echo   scripts\update-github.bat -RemoteUrl git@github.com:USER/REPO.git -Commit -Message "Update launcher" -SetUpstream
    echo.
    echo After origin is configured:
    echo   scripts\update-github.bat -Commit -Message "Update launcher"
    echo.
    echo With no arguments, this script will prompt you for anything it needs.
    echo.
)

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0update-github.ps1" %*
set EXIT_CODE=%ERRORLEVEL%

popd >nul

if not "%EXIT_CODE%"=="0" (
    echo.
    echo update-github failed with exit code %EXIT_CODE%.
    pause
    exit /b %EXIT_CODE%
)

echo.
echo Done.
pause
exit /b 0
