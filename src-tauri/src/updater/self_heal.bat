@echo off
setlocal
:wait
tasklist /FI "PID eq %~1" /NH 2>nul | find " %~1 " >nul
if %errorlevel% equ 0 ( timeout /t 1 /nobreak >nul & goto wait )
timeout /t 1 /nobreak >nul
if /I "%~2"=="%~3" goto launch
if exist "%~3" goto launch_existing
move /y "%~2" "%~3" >nul
goto launch
:launch_existing
start "" "%~3"
goto end
:launch
start "" "%~3"
:end
(goto) 2>nul & del "%~f0"
