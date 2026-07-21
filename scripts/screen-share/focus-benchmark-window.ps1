[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateRange(1, [int]::MaxValue)]
    [int]$ProcessId,

    [ValidateRange(1, 3600)]
    [int]$DurationSeconds = 120,

    [ValidateRange(100, 5000)]
    [int]$IntervalMs = 400
)

$ErrorActionPreference = 'SilentlyContinue'

Add-Type @'
using System;
using System.Runtime.InteropServices;

public static class BenchmarkWindowFocus
{
    [DllImport("user32.dll")]
    public static extern bool ShowWindowAsync(IntPtr window, int command);

    [DllImport("user32.dll")]
    public static extern void SwitchToThisWindow(IntPtr window, bool altTab);
}
'@

$deadline = [DateTime]::UtcNow.AddSeconds($DurationSeconds)
while ([DateTime]::UtcNow -lt $deadline) {
    $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($null -eq $process -or $process.MainWindowHandle -eq 0) {
        break
    }
    [BenchmarkWindowFocus]::ShowWindowAsync($process.MainWindowHandle, 3) | Out-Null
    [BenchmarkWindowFocus]::SwitchToThisWindow($process.MainWindowHandle, $true)
    Start-Sleep -Milliseconds $IntervalMs
}
