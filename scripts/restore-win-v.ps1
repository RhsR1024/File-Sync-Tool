# Emergency recovery script: remove our AllowClipboardHistory override and restart Explorer.
# Usage: Right-click this file -> Run with PowerShell (no elevation required for HKCU).

$ErrorActionPreference = 'Continue'

Remove-ItemProperty `
    -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer" `
    -Name "AllowClipboardHistory" `
    -ErrorAction SilentlyContinue

Write-Host "Registry cleared. Restarting Explorer..." -ForegroundColor Cyan
Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 500
Start-Process explorer.exe

Write-Host "Done. Win+V should now open Windows clipboard history as normal." -ForegroundColor Green
