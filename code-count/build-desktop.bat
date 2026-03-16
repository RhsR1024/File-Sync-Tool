@echo off
REM 编译最终桌面版本 - 支持浏览器退出功能
REM @author l10840, date 2025-09-24

echo Building Code Count Enhanced Desktop Application...

REM 清理之前的编译文件
if exist code-count.exe del code-count.exe

REM 编译为Windows桌面应用（隐藏控制台窗口）
go build -ldflags="-H windowsgui" -o code-count.exe main.go

if exist code-count.exe (
    echo.
    echo ======================================
    echo Build successful!
    echo Executable: code-count.exe
    echo ======================================
    echo.
    echo Enhanced Desktop features:
    echo - System tray icon
    echo - Auto-open browser
    echo - No console window
    echo - Browser close detection
    echo - Exit button in web interface
    echo - Automatic exit when browser closes
    echo.
    echo Double-click code-count.exe to run!
    echo.
    echo Exit methods:
    echo 1. Click "退出程序" button in web interface
    echo 2. Close browser tab/window (auto-exit)
    echo 3. Right-click tray icon -> Exit
    echo.
) else (
    echo.
    echo Build failed! Please check for errors.
    echo.
)

pause