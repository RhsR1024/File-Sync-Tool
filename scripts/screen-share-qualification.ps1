[CmdletBinding()]
param(
    [Parameter()]
    [string]$OutputDirectory = '',

    [Parameter()]
    [ValidateSet('all', 'chrome', 'edge')]
    [string]$Browser = 'all',

    [Parameter()]
    [string]$HostIp = '',

    [Parameter()]
    [switch]$SkipBrowser,

    [Parameter()]
    [switch]$SkipGpu,

    [Parameter()]
    [switch]$SkipBuild,

    [Parameter()]
    [switch]$CollectOnly,

    [Parameter()]
    [ValidateRange(30, 1800)]
    [int]$CommandTimeoutSeconds = 300
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$repositoryRoot = [System.IO.Path]::GetFullPath((Join-Path $scriptRoot '..'))
$tauriRoot = Join-Path $repositoryRoot 'src-tauri'
$timestamp = [DateTimeOffset]::UtcNow
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $fileStamp = $timestamp.ToString('yyyyMMddTHHmmssZ')
    $OutputDirectory = Join-Path $repositoryRoot "artifacts\screen-share-benchmarks\qualification-$fileStamp"
} elseif (-not [System.IO.Path]::IsPathRooted($OutputDirectory)) {
    $OutputDirectory = Join-Path (Get-Location) $OutputDirectory
}
$outputRoot = [System.IO.Path]::GetFullPath($OutputDirectory)
$logsRoot = Join-Path $outputRoot 'logs'
[System.IO.Directory]::CreateDirectory($logsRoot) | Out-Null

function Resolve-NativeExecutable {
    param(
        [Parameter(Mandatory)]
        [string[]]$Names
    )

    foreach ($name in $Names) {
        $command = Get-Command $name -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($null -ne $command) {
            if (-not [string]::IsNullOrWhiteSpace($command.Source)) {
                return $command.Source
            }
            if (-not [string]::IsNullOrWhiteSpace($command.Path)) {
                return $command.Path
            }
        }
    }
    throw "Required executable was not found: $($Names -join ', ')"
}

function ConvertTo-NativeArgument {
    param(
        [AllowEmptyString()]
        [string]$Value
    )

    if ($Value.Length -gt 0 -and $Value -notmatch '[\s"]') {
        return $Value
    }

    $builder = [System.Text.StringBuilder]::new()
    [void]$builder.Append('"')
    $pendingSlashes = 0
    foreach ($character in $Value.ToCharArray()) {
        if ($character -eq '\') {
            $pendingSlashes += 1
            continue
        }
        if ($character -eq '"') {
            for ($index = 0; $index -lt (($pendingSlashes * 2) + 1); $index += 1) {
                [void]$builder.Append('\')
            }
            [void]$builder.Append('"')
            $pendingSlashes = 0
            continue
        }
        for ($index = 0; $index -lt $pendingSlashes; $index += 1) {
            [void]$builder.Append('\')
        }
        $pendingSlashes = 0
        [void]$builder.Append($character)
    }
    for ($index = 0; $index -lt ($pendingSlashes * 2); $index += 1) {
        [void]$builder.Append('\')
    }
    [void]$builder.Append('"')
    return $builder.ToString()
}

function New-SkippedStep {
    param(
        [Parameter(Mandatory)]
        [string]$Name,

        [Parameter(Mandatory)]
        [string]$Reason
    )

    return [ordered]@{
        name = $Name
        status = 'skipped'
        reason = $Reason
        exit_code = $null
        timed_out = $false
        duration_ms = 0
        log_path = $null
    }
}

function Invoke-QualificationStep {
    param(
        [Parameter(Mandatory)]
        [string]$Name,

        [Parameter(Mandatory)]
        [string]$FilePath,

        [Parameter(Mandatory)]
        [string[]]$Arguments,

        [Parameter(Mandatory)]
        [string]$WorkingDirectory,

        [Parameter(Mandatory)]
        [string]$LogPath,

        [Parameter()]
        [hashtable]$Environment = @{},

        [Parameter()]
        [int]$TimeoutSeconds = $CommandTimeoutSeconds
    )

    $start = [DateTimeOffset]::UtcNow
    $processInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $processInfo.FileName = $FilePath
    $processInfo.Arguments = (($Arguments | ForEach-Object { ConvertTo-NativeArgument $_ }) -join ' ')
    $processInfo.WorkingDirectory = $WorkingDirectory
    $processInfo.UseShellExecute = $false
    $processInfo.CreateNoWindow = $true
    $processInfo.RedirectStandardOutput = $true
    $processInfo.RedirectStandardError = $true
    foreach ($entry in $Environment.GetEnumerator()) {
        $processInfo.EnvironmentVariables[$entry.Key] = [string]$entry.Value
    }

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $processInfo
    $started = $process.Start()
    if (-not $started) {
        throw "Failed to start qualification step '$Name'"
    }

    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $completed = $process.WaitForExit($TimeoutSeconds * 1000)
    if (-not $completed) {
        try { $process.Kill() } catch { }
    }
    $process.WaitForExit()
    $stdout = $stdoutTask.GetAwaiter().GetResult()
    $stderr = $stderrTask.GetAwaiter().GetResult()
    $end = [DateTimeOffset]::UtcNow
    $exitCode = if ($completed) { $process.ExitCode } else { $null }
    $status = if (-not $completed) {
        'timed_out'
    } elseif ($exitCode -eq 0) {
        'passed'
    } else {
        'failed'
    }

    $log = @(
        "step=$Name",
        "started_at_utc=$($start.ToString('o'))",
        "finished_at_utc=$($end.ToString('o'))",
        "timeout_seconds=$TimeoutSeconds",
        "status=$status",
        "exit_code=$exitCode",
        '',
        '[stdout]',
        $stdout.TrimEnd(),
        '',
        '[stderr]',
        $stderr.TrimEnd(),
        ''
    ) -join "`n"
    [System.IO.File]::WriteAllText($LogPath, $log, [System.Text.UTF8Encoding]::new($false))

    return [ordered]@{
        name = $Name
        status = $status
        exit_code = $exitCode
        timed_out = -not $completed
        timeout_seconds = $TimeoutSeconds
        duration_ms = [math]::Round(($end - $start).TotalMilliseconds, 3)
        executable = $FilePath
        arguments = @($Arguments)
        working_directory = $WorkingDirectory
        log_path = $LogPath
    }
}

function Read-EncoderCandidateReports {
    param(
        [Parameter(Mandatory)]
        [string]$LogPath
    )

    if (-not (Test-Path -LiteralPath $LogPath -PathType Leaf)) {
        return [ordered]@{ total = 0; parsed = 0; malformed = 0; reports = @() }
    }
    $reports = @()
    $total = 0
    $malformed = 0
    foreach ($line in Get-Content -LiteralPath $LogPath -Encoding UTF8) {
        if ($line -notmatch 'screen-share H\.264 encoder candidate (?:admitted|rejected): (\{.*\})$') {
            continue
        }
        $total += 1
        try {
            $reports += ($Matches[1] | ConvertFrom-Json)
        } catch {
            # Preserve the complete raw log as evidence. A malformed diagnostic
            # line must not abort the remaining host qualification steps.
            $malformed += 1
        }
    }
    return [ordered]@{
        total = $total
        parsed = @($reports).Count
        malformed = $malformed
        reports = @($reports)
    }
}

function Read-GpuPoolRecycleAssertion {
    param([Parameter()][string]$LogPath)
    $result = [ordered]@{ attempted = $false; all_slots_free = $null; pool_size = $null; malformed = 0 }
    if ([string]::IsNullOrWhiteSpace($LogPath) -or -not (Test-Path -LiteralPath $LogPath -PathType Leaf)) {
        return $result
    }
    foreach ($line in Get-Content -LiteralPath $LogPath -Encoding UTF8) {
        if ($line -notmatch 'screen-share H\.264 GPU surface pool recycle assertion: (\{.*\})$') {
            continue
        }
        try {
            $assertion = $Matches[1] | ConvertFrom-Json
            $result.attempted = [bool]$assertion.attempted
            $result.all_slots_free = [bool]$assertion.all_slots_free
            $result.pool_size = $assertion.pool_size
        } catch {
            $result.malformed += 1
        }
    }
    return $result
}

function Get-FileSha256 {
    param([Parameter(Mandatory)][string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $null }
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-RelativeArtifactReference {
    param([Parameter(Mandatory)][string]$Path)
    $baseUri = [System.Uri]::new("$outputRoot$([System.IO.Path]::DirectorySeparatorChar)")
    $targetUri = [System.Uri]::new([System.IO.Path]::GetFullPath($Path))
    return [System.Uri]::UnescapeDataString($baseUri.MakeRelativeUri($targetUri).ToString()).Replace('/', '\\')
}

function New-ArtifactReference {
    param([Parameter(Mandatory)][string]$Path)
    return [ordered]@{
        relative_path = Get-RelativeArtifactReference -Path $Path
        sha256 = Get-FileSha256 -Path $Path
        exists = Test-Path -LiteralPath $Path -PathType Leaf
    }
}

function Invoke-GitText {
    param([Parameter(Mandatory)][string[]]$Arguments)
    try {
        $output = & git -C $repositoryRoot @Arguments 2>$null
        if ($LASTEXITCODE -ne 0) { return $null }
        return ($output -join "`n").Trim()
    } catch { return $null }
}

$powershell = Resolve-NativeExecutable -Names @('powershell.exe', 'pwsh.exe')
$cargo = Resolve-NativeExecutable -Names @('cargo.exe', 'cargo')
$steps = [System.Collections.Generic.List[object]]::new()

$environmentOutput = Join-Path $outputRoot 'environment.json'
$steps.Add((Invoke-QualificationStep `
    -Name 'environment_inventory' `
    -FilePath $powershell `
    -Arguments @(
        '-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
        (Join-Path $scriptRoot 'screen-share-environment.ps1'),
        '-Output', $environmentOutput
    ) `
    -WorkingDirectory $repositoryRoot `
    -LogPath (Join-Path $logsRoot 'environment.log') `
    -TimeoutSeconds ([math]::Min($CommandTimeoutSeconds, 120))))

if ($SkipBuild) {
    $steps.Add((New-SkippedStep -Name 'screen_share_web_build' -Reason 'SkipBuild was requested'))
} else {
    $pnpm = Resolve-NativeExecutable -Names @('pnpm.cmd', 'pnpm.exe')
    $steps.Add((Invoke-QualificationStep `
        -Name 'screen_share_web_build' `
        -FilePath $pnpm `
        -Arguments @('build:screen-share-web') `
        -WorkingDirectory $repositoryRoot `
        -LogPath (Join-Path $logsRoot 'screen-share-web-build.log')))
}

$commonCargoArguments = @('--bin', 'app', '--features', 'screen-share-webrtc-prototype')
$systemMemoryLog = Join-Path $logsRoot 'mf-system-memory-self-test.log'
$steps.Add((Invoke-QualificationStep `
    -Name 'mf_system_memory_self_test' `
    -FilePath $cargo `
    -Arguments (@(
        'test'
    ) + $commonCargoArguments + @(
        'screenshare_media::tests::windows_media_foundation_encoder_passes_startup_self_test',
        '--', '--ignored', '--exact', '--nocapture'
    )) `
    -WorkingDirectory $tauriRoot `
    -LogPath $systemMemoryLog `
    -Environment @{ RUST_LOG = 'info' }))

if ($SkipGpu) {
    $steps.Add((New-SkippedStep -Name 'gpu_dxgi_surface_self_test' -Reason 'SkipGpu was requested'))
    $gpuLog = $null
} else {
    $gpuLog = Join-Path $logsRoot 'gpu-dxgi-surface-self-test.log'
    $steps.Add((Invoke-QualificationStep `
        -Name 'gpu_dxgi_surface_self_test' `
        -FilePath $cargo `
        -Arguments (@(
            'test'
        ) + $commonCargoArguments + @(
            'screenshare_media::tests::windows_gpu_preprocess_and_mf_dxgi_encoder_passes_integration_self_test',
            '--', '--ignored', '--exact', '--nocapture'
        )) `
        -WorkingDirectory $tauriRoot `
        -LogPath $gpuLog `
        -Environment @{ RUST_LOG = 'info' }))
}

$browserOutput = Join-Path $outputRoot 'browser-capabilities.json'
if ($SkipBrowser) {
    $steps.Add((New-SkippedStep -Name 'browser_capability_probe' -Reason 'SkipBrowser was requested'))
} else {
    $node = Resolve-NativeExecutable -Names @('node.exe', 'node')
    $browserArguments = @(
        (Join-Path $scriptRoot 'screen-share-browser-probe.mjs'),
        '--browser', $Browser,
        '--output', $browserOutput
    )
    if (-not [string]::IsNullOrWhiteSpace($HostIp)) {
        $browserArguments += @('--host-ip', $HostIp)
    }
    $steps.Add((Invoke-QualificationStep `
        -Name 'browser_capability_probe' `
        -FilePath $node `
        -Arguments $browserArguments `
        -WorkingDirectory $repositoryRoot `
        -LogPath (Join-Path $logsRoot 'browser-capability-probe.log') `
        -TimeoutSeconds ([math]::Min($CommandTimeoutSeconds, 180))))
}

$failedSteps = @($steps | Where-Object { $_.status -in @('failed', 'timed_out') })
$skippedSteps = @($steps | Where-Object { $_.status -eq 'skipped' })
$qualificationStatus = if ($failedSteps.Count -gt 0) {
    'failed'
} elseif ($skippedSteps.Count -gt 0) {
    'incomplete'
} else {
    'passed'
}
$systemMemoryCandidates = Read-EncoderCandidateReports -LogPath $systemMemoryLog
$gpuCandidates = if ($null -eq $gpuLog) {
    [ordered]@{ total = 0; parsed = 0; malformed = 0; reports = @() }
} else {
    Read-EncoderCandidateReports -LogPath $gpuLog
}
$gpuPoolRecycle = Read-GpuPoolRecycleAssertion -LogPath $gpuLog
$environment = if (Test-Path -LiteralPath $environmentOutput -PathType Leaf) {
    Get-Content -LiteralPath $environmentOutput -Raw -Encoding UTF8 | ConvertFrom-Json
} else { $null }
$package = Get-Content -LiteralPath (Join-Path $repositoryRoot 'package.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$gitCommit = Invoke-GitText -Arguments @('rev-parse', 'HEAD')
$gitStatus = Invoke-GitText -Arguments @('status', '--porcelain')
$hostCpu = if ($null -ne $environment) { @($environment.cpu | Select-Object -First 1) } else { @() }
$hostGpu = if ($null -ne $environment) { @($environment.gpu | Select-Object -First 1) } else { @() }
$hostName = if ($null -ne $environment) { $environment.machine.hostname } else { [System.Net.Dns]::GetHostName() }
$gpuInputCandidates = @($gpuCandidates.reports | Where-Object { $_.gpu_surface_input -and $null -ne $_.input_adapter })
$inputAdapter = if ($gpuInputCandidates.Count -gt 0) { $gpuInputCandidates[0].input_adapter } else { $null }
$activationAdapterLuid = if ($gpuInputCandidates.Count -gt 0) { $gpuInputCandidates[0].activation_adapter_luid } else { $null }
$luidMatch = if (@($gpuInputCandidates | Where-Object { $_.luid_match -eq $true }).Count -gt 0) {
    $true
} elseif (@($gpuInputCandidates | Where-Object { $_.luid_match -eq $false }).Count -gt 0) {
    $false
} else { $null }
$evidenceGaps = [System.Collections.Generic.List[string]]::new()
if ($null -eq $inputAdapter -or [string]::IsNullOrWhiteSpace($inputAdapter.luid)) {
    $evidenceGaps.Add('input_adapter_identity_not_exported_by_current_mf_self_test')
}
if ($null -eq $activationAdapterLuid -or $null -eq $luidMatch) {
    $evidenceGaps.Add('activation_adapter_luid_match_not_observed')
}
if ($gpuPoolRecycle.all_slots_free -ne $true) {
    $evidenceGaps.Add('gpu_surface_pool_recycle_not_observed')
}
$artifacts = [ordered]@{
    environment_report = New-ArtifactReference -Path $environmentOutput
    browser_report = if ($SkipBrowser) { $null } else { New-ArtifactReference -Path $browserOutput }
    step_logs = [ordered]@{}
}
foreach ($step in $steps) {
    if (-not [string]::IsNullOrWhiteSpace($step.log_path)) {
        $artifacts.step_logs[$step.name] = New-ArtifactReference -Path $step.log_path
    }
}

$report = [ordered]@{
    schema_version = 3
    run_id = [guid]::NewGuid().ToString('D')
    generated_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
    qualification_status = $qualificationStatus
    collect_only = [bool]$CollectOnly
    repository_root = $repositoryRoot
    source = [ordered]@{
        git_commit = $gitCommit
        dirty = if ($null -eq $gitStatus) { $null } else { -not [string]::IsNullOrWhiteSpace($gitStatus) }
        app_version = $package.version
        package_manager = $package.packageManager
    }
    host = [ordered]@{
        host_id = $hostName
        hostname = $hostName
        cpu_name = if ($hostCpu.Count -gt 0) { $hostCpu[0].name } else { $null }
        gpu_vendor = if ($hostGpu.Count -gt 0) { $hostGpu[0].adapter_compatibility } else { $null }
        gpu_pnp_device_id = if ($hostGpu.Count -gt 0) { $hostGpu[0].pnp_device_id } else { $null }
        input_adapter = [ordered]@{
            description = if ($null -ne $inputAdapter) { $inputAdapter.description } else { $null }
            vendor_id = if ($null -ne $inputAdapter) { $inputAdapter.vendor_id } else { $null }
            device_id = if ($null -ne $inputAdapter) { $inputAdapter.device_id } else { $null }
            luid = if ($null -ne $inputAdapter) { $inputAdapter.luid } else { $null }
            driver_version = if ($null -ne $inputAdapter) { $inputAdapter.driver_version } else { $null }
            pnp_device_id = if ($null -ne $inputAdapter) { $inputAdapter.pnp_device_id } else { $null }
            evidence_gap = if ($null -eq $inputAdapter) { 'no GPU-surface candidate exported a selected DXGI input adapter' } else { $null }
        }
    }
    environment_report = $environmentOutput
    browser_report = if ($SkipBrowser) { $null } else { $browserOutput }
    media_foundation = [ordered]@{
        # Legacy v2 fields are retained for existing consumers.
        system_memory_candidate_reports = @($systemMemoryCandidates.reports)
        gpu_dxgi_candidate_reports = @($gpuCandidates.reports)
        structured_evidence = [ordered]@{
            system_memory = [ordered]@{
                candidate_total = $systemMemoryCandidates.total
                candidate_parsed = $systemMemoryCandidates.parsed
                candidate_malformed = $systemMemoryCandidates.malformed
                candidates = @($systemMemoryCandidates.reports)
                gate_assertions = [ordered]@{
                    gpu_surface_input = $false
                    input_adapter_identity = $null
                    activation_adapter_luid = $null
                    luid_match = $null
                    pool_recycled = $null
                }
            }
            gpu_dxgi_surface = [ordered]@{
                candidate_total = $gpuCandidates.total
                candidate_parsed = $gpuCandidates.parsed
                candidate_malformed = $gpuCandidates.malformed
                candidates = @($gpuCandidates.reports)
                gate_assertions = [ordered]@{
                    gpu_surface_input = $true
                    input_adapter_identity = ($null -ne $inputAdapter -and -not [string]::IsNullOrWhiteSpace($inputAdapter.luid))
                    activation_adapter_luid = $activationAdapterLuid
                    luid_match = $luidMatch
                    pool_recycled = if ($gpuPoolRecycle.all_slots_free -eq $true) { $true } else { $null }
                    pool_recycle_observation = $gpuPoolRecycle
                }
            }
        }
    }
    artifacts = $artifacts
    evidence_gaps = $evidenceGaps
    steps = @($steps)
    notes = @(
        'A passed system-memory MFT self-test does not imply that DXGI surface input passed.',
        'A failed GPU step is a hardware/driver matrix result even when production safely falls back to CPU/SIMD.',
        'The browser probe is headless constructor/API evidence, not a managed-profile or real-media acceptance test.',
        'Build screen-share Web assets before Cargo because rust-embed reads dist/screen-share-web at compile time.'
    )
}

$reportPath = Join-Path $outputRoot 'qualification.json'
$json = $report | ConvertTo-Json -Depth 10
$null = $json | ConvertFrom-Json
[System.IO.File]::WriteAllText($reportPath, "$json`n", [System.Text.UTF8Encoding]::new($false))

Write-Output $reportPath
Write-Output "qualification_status=$qualificationStatus"
foreach ($step in $steps) {
    Write-Output "$($step.name)=$($step.status)"
}

if ($CollectOnly) {
    exit 0
}
if ($qualificationStatus -eq 'passed') {
    exit 0
}
if ($qualificationStatus -eq 'incomplete') {
    exit 2
}
exit 1
