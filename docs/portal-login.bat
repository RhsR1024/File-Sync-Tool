@echo off
setlocal enabledelayedexpansion

:: ==========================================
::  Portal Auto Login v1.0
::  Entry script: wait network, call login, log results
:: ==========================================

set "SCRIPT_DIR=%~dp0"
set "SCRIPT_DIR=%SCRIPT_DIR:~0,-1%"

set "CONFIG_FILE=%SCRIPT_DIR%\config.ini"

set "LOG_DIR=%SCRIPT_DIR%\logs"
if not exist "%LOG_DIR%" mkdir "%LOG_DIR%"

for /f "tokens=2 delims==" %%a in ('wmic os get localdatetime /value') do set "dt=%%a"
set "LOG_FILE=%LOG_DIR%\login_%dt:~0,8%.log"

:: Read NETWORK_WAIT from config
set "NETWORK_WAIT=30"
if exist "%CONFIG_FILE%" (
    for /f "usebackq tokens=1,* delims==" %%a in ("%CONFIG_FILE%") do (
        for /f "tokens=* delims= " %%k in ("%%a") do (
            for /f "tokens=* delims= " %%v in ("%%b") do (
                if /i "%%k"=="NETWORK_WAIT" set "NETWORK_WAIT=%%v"
            )
        )
    )
)

:: Wait for network
echo [%date% %time:~0,8%] Waiting for network (max %NETWORK_WAIT% seconds)... >> "%LOG_FILE%"
echo [%date% %time:~0,8%] Waiting for network (max %NETWORK_WAIT% seconds)...
set /a waited=0
:wait_network
    if !waited! geq %NETWORK_WAIT% (
        echo [%date% %time:~0,8%] Network wait timeout, trying login anyway... >> "%LOG_FILE%"
        goto :do_login
    )
    ping -n 1 1.1.1.3 >nul 2>&1
    if !errorlevel! equ 0 (
        echo [%date% %time:~0,8%] Network is ready >> "%LOG_FILE%"
        echo [%date% %time:~0,8%] Network is ready
        goto :do_login
    )
    set /a waited+=3
    timeout /t 3 /nobreak >nul 2>&1
    goto :wait_network

:do_login
echo [%date% %time:~0,8%] Starting auto login... >> "%LOG_FILE%"
echo [%date% %time:~0,8%] Starting auto login...

powershell -ExecutionPolicy Bypass -NoProfile -NonInteractive -File "%SCRIPT_DIR%\login.ps1" -ConfigFile "%CONFIG_FILE%"
set "LOGIN_RESULT=%errorlevel%"

if %LOGIN_RESULT% equ 0 (
    echo [%date% %time:~0,8%] Login successful >> "%LOG_FILE%"
    echo [%date% %time:~0,8%] Login successful
) else (
    echo [%date% %time:~0,8%] Login failed, exit code: %LOGIN_RESULT% >> "%LOG_FILE%"
    echo [%date% %time:~0,8%] Login failed, exit code: %LOGIN_RESULT%
)

:: Cleanup old logs
set "RETAIN_DAYS=30"
if exist "%CONFIG_FILE%" (
    for /f "usebackq tokens=1,* delims==" %%a in ("%CONFIG_FILE%") do (
        for /f "tokens=* delims= " %%k in ("%%a") do (
            for /f "tokens=* delims= " %%v in ("%%b") do (
                if /i "%%k"=="LOG_RETAIN_DAYS" set "RETAIN_DAYS=%%v"
            )
        )
    )
)

echo [%date% %time:~0,8%] Cleaning up logs older than %RETAIN_DAYS% days... >> "%LOG_FILE%"
forfiles /p "%LOG_DIR%" /m "login_*.log" /d -%RETAIN_DAYS% /c "cmd /c del /q @path" 2>nul

echo [%date% %time:~0,8%] Script finished >> "%LOG_FILE%"
exit /b %LOGIN_RESULT%
