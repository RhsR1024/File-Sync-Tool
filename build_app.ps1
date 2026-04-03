# Set environment variables
$env:Path = "C:\Users\$env:USERNAME\.cargo\bin;C:\Users\$env:USERNAME\AppData\Roaming\npm;" + $env:Path

Write-Host "Checking environment..." -ForegroundColor Cyan
try {
    cargo --version
    pnpm.cmd --version
} catch {
    Write-Error "Environment check failed."
    exit 1
}

Write-Host "Starting build (Skipping bundle step)..." -ForegroundColor Cyan
# Use --no-bundle to skip the tool download step
& pnpm.cmd tauri build --no-bundle

if ($LASTEXITCODE -eq 0) {
    Write-Host "Build Success!" -ForegroundColor Green
    $exePath = Join-Path (Get-Location) "src-tauri/target/release/app.exe"
    
    if (Test-Path $exePath) {
        Write-Host "Executable location: $exePath" -ForegroundColor Green
        Write-Host "Note: This is a standalone executable." -ForegroundColor Yellow
        # Open folder and select file
        explorer /select,$exePath
    } else {
        Write-Error "Executable not found."
    }
} else {
    Write-Error "Build failed."
}
