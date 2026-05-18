@echo off
setlocal
:wait
tasklist /FI "PID eq %~1" /NH 2>nul | find " %~1 " >nul
if %errorlevel% equ 0 ( timeout /t 1 /nobreak >nul & goto wait )
timeout /t 1 /nobreak >nul
if /I "%~2"=="%~4" goto skip_target_backup
if exist "%~4.old" del /f /q "%~4.old" 2>nul
if exist "%~4" move /y "%~4" "%~4.old" >nul 2>nul
:skip_target_backup
if /I "%~3"=="%~4" goto install
if exist "%~3.old" del /f /q "%~3.old" 2>nul
if exist "%~3" move /y "%~3" "%~3.old" >nul 2>nul
:install
if /I "%~2"=="%~4" goto launch
move /y "%~2" "%~4" >nul
:launch
start "" "%~4"
(goto) 2>nul & del "%~f0"
