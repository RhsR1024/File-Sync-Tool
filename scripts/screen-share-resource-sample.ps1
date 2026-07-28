# Samples the sharing host's process CPU, memory and GPU utilization for the
# `resource_usage` fields of the performance-matrix field-evidence report
# (screen-share-latency-optimization.md 8.2). It measures one host process; the
# viewer side must be measured on the viewing device.
[CmdletBinding()]
param(
    [Parameter()]
    [string]$ProcessName = 'app',

    [Parameter()]
    [int]$ProcessId = 0,

    [Parameter()]
    [int]$DurationSeconds = 60,

    [Parameter()]
    [double]$IntervalSeconds = 1,

    # Enumerating every GPU engine instance costs seconds on some drivers, so GPU
    # is sampled on its own slower cadence and never starves CPU/memory sampling.
    [Parameter()]
    [double]$GpuIntervalSeconds = 5,

    [Parameter()]
    [string]$Label = '',

    [Parameter()]
    [string]$Output = ''
)

$ErrorActionPreference = 'Stop'
$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$repositoryRoot = [System.IO.Path]::GetFullPath((Join-Path $scriptRoot '..'))
$timestamp = [DateTimeOffset]::UtcNow

if ($DurationSeconds -le 0) {
    Write-Error 'DurationSeconds must be greater than zero.'
    exit 3
}
if ($IntervalSeconds -le 0) {
    Write-Error 'IntervalSeconds must be greater than zero.'
    exit 3
}

if ([string]::IsNullOrWhiteSpace($Output)) {
    $fileStamp = $timestamp.ToString('yyyyMMddTHHmmssZ')
    $Output = Join-Path $repositoryRoot "artifacts\screen-share-benchmarks\resource-usage-$fileStamp.json"
} elseif (-not [System.IO.Path]::IsPathRooted($Output)) {
    $Output = Join-Path (Get-Location) $Output
}
$outputPath = [System.IO.Path]::GetFullPath($Output)

function Resolve-TargetProcess {
    if ($ProcessId -gt 0) {
        try {
            return Get-Process -Id $ProcessId -ErrorAction Stop
        } catch {
            return $null
        }
    }
    $candidates = @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue)
    if ($candidates.Count -eq 0) { return $null }
    # The sharing host runs one instance; if several match, take the one using
    # the most CPU so a stale background copy cannot silence the measurement.
    return ($candidates | Sort-Object -Property TotalProcessorTime -Descending | Select-Object -First 1)
}

$process = Resolve-TargetProcess
if ($null -eq $process) {
    Write-Error "No running process matched (name='$ProcessName', pid=$ProcessId). Start the share first."
    exit 1
}

$logicalProcessors = [Environment]::ProcessorCount
if ($logicalProcessors -le 0) { $logicalProcessors = 1 }

function Get-GpuUtilizationPercent {
    param(
        [Parameter(Mandatory)]
        [int]$TargetProcessId
    )
    try {
        $counter = Get-Counter -Counter '\GPU Engine(*)\Utilization Percentage' -ErrorAction Stop
        $matching = @($counter.CounterSamples | Where-Object { $_.InstanceName -like "pid_${TargetProcessId}_*" })
        if ($matching.Count -eq 0) { return 0.0 }
        return [math]::Round((($matching | Measure-Object -Property CookedValue -Sum).Sum), 3)
    } catch {
        # GPU engine counters are unavailable on some drivers and older builds.
        return $null
    }
}

function Get-Percentile {
    param(
        [Parameter()]
        [double[]]$Values,
        [Parameter(Mandatory)]
        [double]$Fraction
    )
    if ($null -eq $Values -or $Values.Count -eq 0) { return $null }
    $sorted = @($Values | Sort-Object)
    $index = [int][math]::Ceiling($sorted.Count * $Fraction) - 1
    if ($index -lt 0) { $index = 0 }
    if ($index -ge $sorted.Count) { $index = $sorted.Count - 1 }
    return [math]::Round($sorted[$index], 3)
}

function ConvertTo-Distribution {
    param(
        [Parameter()]
        [double[]]$Values,
        [Parameter(Mandatory)]
        [string]$MeasurementScope
    )
    $clean = @($Values | Where-Object { $null -ne $_ })
    if ($clean.Count -eq 0) {
        return [ordered]@{
            p50 = $null; p95 = $null; p99 = $null; max = $null
            sample_count = 0; retained_sample_count = 0; capacity = 0
            measurement_scope = $MeasurementScope
        }
    }
    return [ordered]@{
        p50 = Get-Percentile -Values $clean -Fraction 0.5
        p95 = Get-Percentile -Values $clean -Fraction 0.95
        p99 = Get-Percentile -Values $clean -Fraction 0.99
        max = [math]::Round((($clean | Measure-Object -Maximum).Maximum), 3)
        sample_count = $clean.Count
        retained_sample_count = $clean.Count
        capacity = $clean.Count
        measurement_scope = $MeasurementScope
    }
}

$targetPid = $process.Id
$processPath = try { $process.Path } catch { $null }
$startedAt = [DateTimeOffset]::UtcNow
$cpuSamples = New-Object System.Collections.Generic.List[double]
$workingSetSamples = New-Object System.Collections.Generic.List[double]
$privateBytesSamples = New-Object System.Collections.Generic.List[double]
$handleSamples = New-Object System.Collections.Generic.List[double]
$gpuSamples = New-Object System.Collections.Generic.List[double]
$gpuCounterAvailable = $true
$exitedEarly = $false

$previousCpu = $process.TotalProcessorTime
$previousAt = [DateTimeOffset]::UtcNow
$deadline = $previousAt.AddSeconds($DurationSeconds)
$nextTickAt = $previousAt
$lastGpuSampleAt = [DateTimeOffset]::MinValue

while ([DateTimeOffset]::UtcNow -lt $deadline) {
    # Sleep to the next fixed tick instead of a flat interval; otherwise a slow
    # counter read silently stretches the sampling period.
    $nextTickAt = $nextTickAt.AddSeconds($IntervalSeconds)
    $waitMilliseconds = ($nextTickAt - [DateTimeOffset]::UtcNow).TotalMilliseconds
    if ($waitMilliseconds -gt 0) { Start-Sleep -Milliseconds ([int]$waitMilliseconds) }
    try {
        $process.Refresh()
        if ($process.HasExited) { $exitedEarly = $true; break }
    } catch {
        $exitedEarly = $true
        break
    }

    $now = [DateTimeOffset]::UtcNow
    $elapsedSeconds = ($now - $previousAt).TotalSeconds
    if ($elapsedSeconds -le 0) { continue }
    $cpuDelta = ($process.TotalProcessorTime - $previousCpu).TotalSeconds
    $previousCpu = $process.TotalProcessorTime
    $previousAt = $now

    $cpuPercent = [math]::Round((($cpuDelta / $elapsedSeconds) / $logicalProcessors) * 100, 3)
    if ($cpuPercent -lt 0) { $cpuPercent = 0 }
    $cpuSamples.Add($cpuPercent)
    $workingSetSamples.Add([math]::Round($process.WorkingSet64 / 1MB, 3))
    $privateBytesSamples.Add([math]::Round($process.PrivateMemorySize64 / 1MB, 3))
    $handleSamples.Add([double]$process.HandleCount)

    if ($gpuCounterAvailable -and ($now - $lastGpuSampleAt).TotalSeconds -ge $GpuIntervalSeconds) {
        $lastGpuSampleAt = [DateTimeOffset]::UtcNow
        $gpu = Get-GpuUtilizationPercent -TargetProcessId $targetPid
        if ($null -eq $gpu) { $gpuCounterAvailable = $false } else { $gpuSamples.Add($gpu) }
    }
}

$completedAt = [DateTimeOffset]::UtcNow
$evidenceGaps = @()
if ($cpuSamples.Count -eq 0) { $evidenceGaps += 'no CPU samples were collected' }
if (-not $gpuCounterAvailable -or $gpuSamples.Count -eq 0) {
    $evidenceGaps += 'GPU engine counters were unavailable for this process; host_gpu_percent must come from another tool'
}
if ($exitedEarly) { $evidenceGaps += 'the target process exited before the requested duration elapsed' }

$report = [ordered]@{
    schema_version = 1
    scope = 'host-resource-usage'
    status = if ($evidenceGaps.Count -eq 0) { 'passed' } else { 'incomplete' }
    run_id = if ([string]::IsNullOrWhiteSpace($Label)) { "resource-$($startedAt.ToString('yyyyMMddTHHmmssZ'))" } else { $Label }
    spec_completion = 'not_evaluated'
    host = [ordered]@{
        machine_name = $env:COMPUTERNAME
        logical_processors = $logicalProcessors
    }
    process = [ordered]@{
        id = $targetPid
        name = $process.ProcessName
        path = $processPath
        exited_early = $exitedEarly
    }
    window = [ordered]@{
        started_at_utc = $startedAt.ToString('o')
        completed_at_utc = $completedAt.ToString('o')
        requested_duration_seconds = $DurationSeconds
        interval_seconds = $IntervalSeconds
        gpu_interval_seconds = $GpuIntervalSeconds
        observed_duration_seconds = [math]::Round(($completedAt - $startedAt).TotalSeconds, 3)
    }
    host_cpu_percent = ConvertTo-Distribution -Values $cpuSamples.ToArray() -MeasurementScope 'process_total_cpu_normalized_by_logical_processors'
    host_gpu_percent = ConvertTo-Distribution -Values $gpuSamples.ToArray() -MeasurementScope 'gpu_engine_utilization_sum_for_process'
    host_working_set_mb = ConvertTo-Distribution -Values $workingSetSamples.ToArray() -MeasurementScope 'process_working_set'
    host_private_memory_mb = ConvertTo-Distribution -Values $privateBytesSamples.ToArray() -MeasurementScope 'process_private_bytes'
    host_handle_count = ConvertTo-Distribution -Values $handleSamples.ToArray() -MeasurementScope 'process_handle_count'
    evidence_gaps = @($evidenceGaps)
    notes = @(
        'This artifact covers one host process only; viewer CPU/GPU/memory must be measured on the viewing device.',
        'GPU utilization is the sum of that process GPU engine counters and is not a per-adapter capacity measurement.',
        'Copy these percentiles into the performance-matrix run resource_usage fields; the sampler does not decide any gate.'
    )
}

$directory = Split-Path -Parent $outputPath
[System.IO.Directory]::CreateDirectory($directory) | Out-Null
$json = $report | ConvertTo-Json -Depth 8
$null = $json | ConvertFrom-Json
[System.IO.File]::WriteAllText($outputPath, "$json`n", [System.Text.UTF8Encoding]::new($false))
Write-Output $outputPath
if ($evidenceGaps.Count -gt 0) { exit 2 }
exit 0
