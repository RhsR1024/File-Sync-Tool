@echo off
setlocal EnableExtensions EnableDelayedExpansion
set "LOG_FILE=%TEMP%\file-sync-tool-updater.log"
set /a WAIT_COUNT=0
>>"%LOG_FILE%" echo [%date% %time%] self-heal helper started; pid=%~1 source=%~2 target=%~3

:wait
tasklist /FI "PID eq %~1" /NH 2>nul | find " %~1 " >nul
if errorlevel 1 goto process_stopped
set /a WAIT_COUNT+=1
if !WAIT_COUNT! geq 15 goto process_still_running
timeout /t 1 /nobreak >nul
goto wait

:process_still_running
>>"%LOG_FILE%" echo [%date% %time%] old process did not exit after !WAIT_COUNT! seconds; self-heal was not applied
goto failed

:process_stopped
if not exist "%~2" (
  >>"%LOG_FILE%" echo [%date% %time%] self-heal source is missing: %~2
  goto failed
)
if /I "%~2"=="%~3" goto launch
if exist "%~3" goto launch
move /y "%~2" "%~3" >>"%LOG_FILE%" 2>&1
if errorlevel 1 goto failed
goto launch

:launch
pushd "%~dp3"
start "" "%~3"
set "LAUNCH_ERROR=!errorlevel!"
popd
if not "!LAUNCH_ERROR!"=="0" goto launch_failed
>>"%LOG_FILE%" echo [%date% %time%] self-heal target launched successfully: %~3
goto cleanup

:launch_failed
>>"%LOG_FILE%" echo [%date% %time%] failed to launch self-heal target; error=!LAUNCH_ERROR! target=%~3
if /I not "%~2"=="%~3" if exist "%~3" move /y "%~3" "%~2" >>"%LOG_FILE%" 2>&1

:failed
>>"%LOG_FILE%" echo [%date% %time%] self-heal helper failed; relaunching source when available: %~2
if exist "%~2" (
  pushd "%~dp2"
  start "" "%~2"
  popd
)

:cleanup
(goto) 2>nul & del "%~f0"
