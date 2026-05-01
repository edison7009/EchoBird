@echo off
setlocal EnableExtensions

cd /d "%~dp0"
title EchoBird Repo Dev

echo.
echo ============================================
echo   EchoBird Repo Dev
echo ============================================
echo.

where node >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Node.js was not found in PATH.
  echo Please install Node.js first.
  echo.
  pause
  exit /b 1
)

where npm >nul 2>nul
if errorlevel 1 (
  echo [ERROR] npm was not found in PATH.
  echo Please install Node.js/npm first.
  echo.
  pause
  exit /b 1
)

where cargo >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Rust cargo was not found in PATH.
  echo Please install Rust first.
  echo.
  pause
  exit /b 1
)

if not exist "node_modules" (
  echo [INFO] node_modules not found. Running npm install...
  call npm install
  if errorlevel 1 (
    echo.
    echo [ERROR] npm install failed.
    pause
    exit /b 1
  )
  echo.
)

echo [INFO] Starting repo in original dev mode...
echo [INFO] Command: npm run dev
echo.
call npm run dev

set EXIT_CODE=%ERRORLEVEL%
echo.
if not "%EXIT_CODE%"=="0" (
  echo [ERROR] Process exited with code %EXIT_CODE%.
) else (
  echo [INFO] Process exited normally.
)
echo.
pause
exit /b %EXIT_CODE%
