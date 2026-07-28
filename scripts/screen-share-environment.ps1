[CmdletBinding()]
param(
    [Parameter()]
    [string]$Output = ''
)

$ErrorActionPreference = 'Stop'
$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$repositoryRoot = [System.IO.Path]::GetFullPath((Join-Path $scriptRoot '..'))
$timestamp = [DateTimeOffset]::UtcNow
if ([string]::IsNullOrWhiteSpace($Output)) {
    $fileStamp = $timestamp.ToString('yyyyMMddTHHmmssZ')
    $Output = Join-Path $repositoryRoot "artifacts\screen-share-benchmarks\environment-$fileStamp.json"
} elseif (-not [System.IO.Path]::IsPathRooted($Output)) {
    $Output = Join-Path (Get-Location) $Output
}
$outputPath = [System.IO.Path]::GetFullPath($Output)

function Invoke-OptionalCommand {
    param(
        [Parameter(Mandatory)]
        [scriptblock]$Action
    )
    try {
        return & $Action
    } catch {
        return $null
    }
}

function Invoke-GitText {
    param(
        [Parameter(Mandatory)]
        [string[]]$Arguments
    )
    try {
        $output = & git -C $repositoryRoot @Arguments 2>$null
        if ($LASTEXITCODE -ne 0) { return $null }
        return ($output -join "`n").Trim()
    } catch {
        return $null
    }
}

function Get-ExecutableVersion {
    param(
        [Parameter(Mandatory)]
        [string[]]$Candidates
    )
    foreach ($candidate in $Candidates) {
        if ([string]::IsNullOrWhiteSpace($candidate) -or -not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
            continue
        }
        $item = Get-Item -LiteralPath $candidate
        return [ordered]@{
            path = $item.FullName
            version = $item.VersionInfo.ProductVersion
        }
    }
    return $null
}

$operatingSystem = Invoke-OptionalCommand { Get-CimInstance Win32_OperatingSystem }
$computerSystem = Invoke-OptionalCommand { Get-CimInstance Win32_ComputerSystem }
$processors = @(Invoke-OptionalCommand { Get-CimInstance Win32_Processor } | Where-Object { $null -ne $_ })
$videoControllers = @(Invoke-OptionalCommand { Get-CimInstance Win32_VideoController } | Where-Object { $null -ne $_ })
$desktopMonitors = @(Invoke-OptionalCommand { Get-CimInstance Win32_DesktopMonitor } | Where-Object { $null -ne $_ })
$packageJson = Invoke-OptionalCommand {
    Get-Content -LiteralPath (Join-Path $repositoryRoot 'package.json') -Raw -Encoding UTF8 | ConvertFrom-Json
}
$gitCommit = Invoke-GitText -Arguments @('rev-parse', 'HEAD')
$gitBranch = Invoke-GitText -Arguments @('branch', '--show-current')
$gitStatus = Invoke-GitText -Arguments @('status', '--porcelain')
$edge = Get-ExecutableVersion -Candidates @(
    (Join-Path ${env:ProgramFiles(x86)} 'Microsoft\Edge\Application\msedge.exe'),
    (Join-Path $env:ProgramFiles 'Microsoft\Edge\Application\msedge.exe')
)
$chrome = Get-ExecutableVersion -Candidates @(
    (Join-Path ${env:ProgramFiles(x86)} 'Google\Chrome\Application\chrome.exe'),
    (Join-Path $env:ProgramFiles 'Google\Chrome\Application\chrome.exe'),
    (Join-Path $env:LOCALAPPDATA 'Google\Chrome\Application\chrome.exe')
)
$edgePolicyScopes = @(
    'HKLM:\Software\Policies\Microsoft\Edge',
    'HKCU:\Software\Policies\Microsoft\Edge'
) | Where-Object { Test-Path -LiteralPath $_ }
$edgePolicyInventory = @($edgePolicyScopes | ForEach-Object {
    $scope = $_
    $properties = Invoke-OptionalCommand { Get-ItemProperty -LiteralPath $scope }
    [ordered]@{
        scope = $scope
        # Policy values may contain internal URLs or identifiers. Preserve only
        # names here; the target-site test owner can inspect values locally.
        value_names = @($properties.PSObject.Properties.Name | Where-Object {
            $_ -notlike 'PS*'
        } | Sort-Object)
    }
})

$gpu = @($videoControllers | ForEach-Object {
    [ordered]@{
        name = $_.Name
        adapter_compatibility = $_.AdapterCompatibility
        video_processor = $_.VideoProcessor
        pnp_device_id = $_.PNPDeviceID
        driver_version = $_.DriverVersion
        driver_date = if ($_.DriverDate) { ([DateTime]$_.DriverDate).ToUniversalTime().ToString('o') } else { $null }
        adapter_ram_bytes = if ($null -ne $_.AdapterRAM) { [uint64]$_.AdapterRAM } else { $null }
        current_horizontal_resolution = $_.CurrentHorizontalResolution
        current_vertical_resolution = $_.CurrentVerticalResolution
        current_refresh_rate_hz = $_.CurrentRefreshRate
        status = $_.Status
    }
})

$report = [ordered]@{
    schema_version = 1
    generated_at_utc = $timestamp.ToString('o')
    purpose = 'screen-share target-host hardware and driver evidence'
    source = [ordered]@{
        app_version = $packageJson.version
        git_commit = $gitCommit
        git_branch = $gitBranch
        git_worktree_dirty = if ($null -eq $gitStatus) { $null } else { -not [string]::IsNullOrWhiteSpace($gitStatus) }
    }
    windows = [ordered]@{
        caption = $operatingSystem.Caption
        version = $operatingSystem.Version
        build_number = $operatingSystem.BuildNumber
        architecture = $operatingSystem.OSArchitecture
        last_boot_utc = if ($operatingSystem.LastBootUpTime) {
            ([DateTime]$operatingSystem.LastBootUpTime).ToUniversalTime().ToString('o')
        } else { $null }
        session_name = $env:SESSIONNAME
        remote_desktop_session = [bool]($env:SESSIONNAME -like 'RDP-*')
    }
    machine = [ordered]@{
        hostname = [System.Net.Dns]::GetHostName()
        manufacturer = $computerSystem.Manufacturer
        model = $computerSystem.Model
        total_physical_memory_bytes = if ($null -ne $computerSystem.TotalPhysicalMemory) {
            [uint64]$computerSystem.TotalPhysicalMemory
        } else { $null }
        hypervisor_present = $computerSystem.HypervisorPresent
    }
    cpu = @($processors | ForEach-Object {
        [ordered]@{
            name = $_.Name
            manufacturer = $_.Manufacturer
            device_id = $_.DeviceID
            cores = $_.NumberOfCores
            logical_processors = $_.NumberOfLogicalProcessors
            max_clock_mhz = $_.MaxClockSpeed
        }
    })
    gpu = $gpu
    displays = @($desktopMonitors | ForEach-Object {
        [ordered]@{
            name = $_.Name
            pnp_device_id = $_.PNPDeviceID
            screen_width = $_.ScreenWidth
            screen_height = $_.ScreenHeight
            status = $_.Status
        }
    })
    browsers = [ordered]@{
        edge = $edge
        chrome = $chrome
        edge_managed_policy_scopes = @($edgePolicyScopes)
        edge_managed_policy_inventory = $edgePolicyInventory
    }
    classifications = [ordered]@{
        microsoft_basic_display_adapter = [bool]($gpu | Where-Object {
            $_.name -match 'Microsoft Basic Display|微软基本显示'
        })
        virtual_or_remote_adapter = [bool]($gpu | Where-Object {
            $_.name -match 'Remote|Virtual|VMware|Hyper-V|Citrix|Parallels'
        })
    }
    notes = @(
        'Run this script on the sharing host, not only on the benchmark client.',
        'This inventory does not prove GPU pipeline activation; correlate it with /status h264_media and media_metrics.',
        'Review hostname and PNP device identifiers before sharing the artifact outside the test organization.'
    )
}

$directory = Split-Path -Parent $outputPath
[System.IO.Directory]::CreateDirectory($directory) | Out-Null
$json = $report | ConvertTo-Json -Depth 8
$null = $json | ConvertFrom-Json
[System.IO.File]::WriteAllText($outputPath, "$json`n", [System.Text.UTF8Encoding]::new($false))
Write-Output $outputPath
