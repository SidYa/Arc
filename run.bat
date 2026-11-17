@echo off
rem Arc: 1 - run in development mode, 2 - build the NSIS installer.
chcp 65001 >nul
cd /d "%~dp0app"

echo.
echo   Arc
echo   1. Run (dev)
echo   2. Build installer (NSIS)
echo.
choice /c 12 /n /m "Choose 1 or 2: "
if errorlevel 2 goto build

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0app\dev.ps1"
goto end

:build
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0app\dev.ps1" -Build
if not errorlevel 1 (
    echo.
    echo Installer:
    dir /b /s "%~dp0app\src-tauri\target\release\bundle\nsis\*.exe"
    explorer "%~dp0app\src-tauri\target\release\bundle\nsis"
)

:end
echo.
pause
