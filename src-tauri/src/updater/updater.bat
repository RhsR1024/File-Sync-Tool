@echo off
setlocal EnableExtensions EnableDelayedExpansion
set "LOG_FILE=%TEMP%\file-sync-tool-updater.log"
set /a WAIT_COUNT=0
set "TARGET_BACKED_UP=0"
>>"%LOG_FILE%" echo [%date% %time%] update helper started; pid=%~1 source=%~2 current=%~3 target=%~4

:wait
tasklist /FI "PID eq %~1" /NH 2>nul | find " %~1 " >nul
if errorlevel 1 goto process_stopped
set /a WAIT_COUNT+=1
if !WAIT_COUNT! geq 15 goto process_still_running
timeout /t 1 /nobreak >nul
goto wait

:process_still_running
>>"%LOG_FILE%" echo [%date% %time%] old process did not exit after !WAIT_COUNT! seconds; update was not installed
goto failed

:process_stopped
if not exist "%~2" (
  >>"%LOG_FILE%" echo [%date% %time%] update source is missing: %~2
  goto failed
)
if /I "%~2"=="%~4" goto launch

if exist "%~4" (
  move /y "%~4" "%~4.old" >>"%LOG_FILE%" 2>&1
  if errorlevel 1 goto failed
  set "TARGET_BACKED_UP=1"
)
move /y "%~2" "%~4" >>"%LOG_FILE%" 2>&1
if errorlevel 1 goto install_failed
goto launch

:install_failed
>>"%LOG_FILE%" echo [%date% %time%] failed to install update target: %~4
if "!TARGET_BACKED_UP!"=="1" if not exist "%~4" move /y "%~4.old" "%~4" >>"%LOG_FILE%" 2>&1
goto failed

:launch
if not exist "%~4" (
  >>"%LOG_FILE%" echo [%date% %time%] update target is missing before launch: %~4
  goto failed
)
pushd "%~dp4"
start "" "%~4"
set "LAUNCH_ERROR=!errorlevel!"
popd
if not "!LAUNCH_ERROR!"=="0" (
  >>"%LOG_FILE%" echo [%date% %time%] failed to launch update target; error=!LAUNCH_ERROR! target=%~4
  goto launch_failed
)
>>"%LOG_FILE%" echo [%date% %time%] update target launched successfully: %~4
goto cleanup

:launch_failed
if "!TARGET_BACKED_UP!"=="1" (
  move /y "%~4" "%~2" >>"%LOG_FILE%" 2>&1
  if not errorlevel 1 move /y "%~4.old" "%~4" >>"%LOG_FILE%" 2>&1
)
goto failed

:failed
>>"%LOG_FILE%" echo [%date% %time%] update helper failed; the previous executable remains at: %~3
if exist "%~3" (
  pushd "%~dp3"
  start "" "%~3"
  popd
)

:cleanup
(goto) 2>nul & del "%~f0"
