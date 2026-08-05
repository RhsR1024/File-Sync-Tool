[CmdletBinding()]
param(
    [string]$ToolExe,
    [string]$SourceDirectory = (Join-Path $PSScriptRoot 'virtual-device-assets\source-videos'),
    [string]$OutputDirectory = (Join-Path $PSScriptRoot 'virtual-device-assets\prepared-videos'),
    [string]$DefaultVideo
)

$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($ToolExe)) {
    $candidate = Get-ChildItem -LiteralPath $PSScriptRoot -File -Filter 'file-sync-tool-*.exe' |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 1
    if ($null -eq $candidate) {
        throw 'No file-sync-tool-*.exe was found. Pass the new bare EXE with -ToolExe.'
    }
    $ToolExe = $candidate.FullName
}

$resolvedTool = (Resolve-Path -LiteralPath $ToolExe).Path
$resolvedSource = (Resolve-Path -LiteralPath $SourceDirectory).Path
$sourceVideos = @(Get-ChildItem -LiteralPath $resolvedSource -File -Filter '*.mp4')
if ($sourceVideos.Count -eq 0) {
    throw "No MP4 files were found in: $resolvedSource"
}

$ffmpegBesideTool = Join-Path (Split-Path -Parent $resolvedTool) 'ffmpeg.exe'
$ffmpegOnPath = Get-Command ffmpeg.exe -ErrorAction SilentlyContinue
if (-not (Test-Path -LiteralPath $ffmpegBesideTool -PathType Leaf) -and $null -eq $ffmpegOnPath) {
    throw 'ffmpeg.exe was not found. Put it beside the tool EXE or add it to PATH on the upgrade server.'
}

$resolvedOutput = [System.IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $resolvedOutput | Out-Null

Write-Host "Preparing $($sourceVideos.Count) MP4 files. Unchanged videos will reuse existing output."
$toolArguments = @(
    '--prepare-device-simulator-materials',
    ('"' + $resolvedSource + '"'),
    ('"' + $resolvedOutput + '"')
)
if (-not [string]::IsNullOrWhiteSpace($DefaultVideo)) {
    $toolArguments += ('"' + $DefaultVideo + '"')
}
$process = Start-Process -FilePath $resolvedTool `
    -ArgumentList $toolArguments `
    -WindowStyle Hidden `
    -Wait `
    -PassThru
if ($process.ExitCode -ne 0) {
    throw "Material preparation failed with exit code $($process.ExitCode)."
}

$catalog = Join-Path $resolvedOutput 'prepared-catalog.json'
if (-not (Test-Path -LiteralPath $catalog -PathType Leaf)) {
    throw "Preparation finished without producing: $catalog"
}

Write-Host 'Material preparation completed. serve.py discovers the output automatically.'
Write-Host 'Clients can now use Sync from server without FFmpeg.'
Write-Host "Published directory: $resolvedOutput"
