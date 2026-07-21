[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$BaseUrl,

    [ValidateSet('static', 'scroll', 'video')]
    [string[]]$Scenario = @('static'),

    [ValidateRange(1, 200)]
    [int[]]$ViewerCount = @(1, 5, 10, 50),

    [ValidateRange(5, 3600)]
    [int]$DurationSeconds = 30,

    [ValidateRange(0, 300)]
    [int]$WarmupSeconds = 5,

    [ValidateRange(250, 10000)]
    [int]$SampleIntervalMs = 1000,

    [int]$ProcessId = 0,

    [int]$VisualProcessId = 0,

    [switch]$SyntheticVisual,

    [string]$Username = '',

    [string]$Password = '',

    [string]$OutputDirectory = 'artifacts/screen-share-benchmarks',

    [switch]$NonInteractive
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Net.Http
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;

public static class ScreenShareBenchmarkFocus
{
    [DllImport("user32.dll")]
    public static extern bool ShowWindowAsync(IntPtr window, int command);

    [DllImport("user32.dll")]
    public static extern void SwitchToThisWindow(IntPtr window, bool altTab);
}
'@

Add-Type -ReferencedAssemblies @('System.Drawing', 'System.Windows.Forms') -TypeDefinition @'
using System;
using System.Drawing;
using System.Threading;
using System.Windows.Forms;

public static class ScreenShareSyntheticVisual
{
    private static readonly object Sync = new object();
    private static Thread thread;
    private static SyntheticVisualForm form;
    private static ManualResetEvent ready;

    public static void Start(string mode)
    {
        Stop();
        lock (Sync)
        {
            ready = new ManualResetEvent(false);
            thread = new Thread(() =>
            {
                var next = new SyntheticVisualForm(mode);
                lock (Sync) { form = next; }
                next.Shown += (_, __) => ready.Set();
                Application.Run(next);
                lock (Sync) { form = null; }
            });
            thread.IsBackground = true;
            thread.Name = "screen-share-benchmark-visual";
            thread.SetApartmentState(ApartmentState.STA);
            thread.Start();
        }
        if (!ready.WaitOne(TimeSpan.FromSeconds(5)))
        {
            Stop();
            throw new InvalidOperationException("Synthetic visual window did not start.");
        }
    }

    public static void Stop()
    {
        SyntheticVisualForm current;
        Thread currentThread;
        lock (Sync)
        {
            current = form;
            currentThread = thread;
        }
        if (current != null && !current.IsDisposed)
        {
            try { current.BeginInvoke(new Action(current.Close)); } catch { }
        }
        if (currentThread != null && currentThread.IsAlive)
        {
            currentThread.Join(TimeSpan.FromSeconds(3));
        }
        lock (Sync)
        {
            thread = null;
            form = null;
            if (ready != null) ready.Dispose();
            ready = null;
        }
    }

    private sealed class SyntheticVisualForm : Form
    {
        private readonly string mode;
        private readonly System.Windows.Forms.Timer timer;
        private readonly Brush[] palette;
        private readonly Font titleFont;
        private readonly Font bodyFont;
        private readonly Pen borderPen;
        private int tick;

        public SyntheticVisualForm(string mode)
        {
            this.mode = mode == "scroll" ? "scroll" : "video";
            FormBorderStyle = FormBorderStyle.None;
            WindowState = FormWindowState.Maximized;
            StartPosition = FormStartPosition.Manual;
            Bounds = Screen.PrimaryScreen.Bounds;
            TopMost = true;
            ShowInTaskbar = false;
            BackColor = Color.FromArgb(11, 13, 16);
            DoubleBuffered = true;
            palette = new Brush[]
            {
                new SolidBrush(Color.FromArgb(17, 24, 39)),
                new SolidBrush(Color.FromArgb(239, 68, 68)),
                new SolidBrush(Color.FromArgb(245, 158, 11)),
                new SolidBrush(Color.FromArgb(34, 197, 94)),
                new SolidBrush(Color.FromArgb(56, 189, 248)),
                new SolidBrush(Color.FromArgb(248, 250, 252)),
            };
            titleFont = new Font("Segoe UI", 18, FontStyle.Bold, GraphicsUnit.Pixel);
            bodyFont = new Font("Consolas", 14, FontStyle.Regular, GraphicsUnit.Pixel);
            borderPen = new Pen(Color.FromArgb(71, 85, 105), 1);
            timer = new System.Windows.Forms.Timer { Interval = 33 };
            timer.Tick += (_, __) => { tick++; Invalidate(); };
            timer.Start();
        }

        protected override void OnPaint(PaintEventArgs args)
        {
            base.OnPaint(args);
            if (mode == "scroll") DrawScroll(args.Graphics);
            else DrawVideo(args.Graphics);
        }

        private void DrawScroll(Graphics graphics)
        {
            graphics.Clear(Color.FromArgb(11, 13, 16));
            const int rowHeight = 54;
            int offset = (tick * 4) % rowHeight;
            int first = (tick * 4) / rowHeight;
            for (int y = -rowHeight + offset, row = first; y < ClientSize.Height; y += rowHeight, row++)
            {
                graphics.FillRectangle(palette[row % 2 == 0 ? 0 : 3], 0, y, ClientSize.Width, rowHeight - 2);
                graphics.DrawRectangle(borderPen, 0, y, ClientSize.Width - 1, rowHeight - 2);
                string state = new[] { "RUNNING", "VERIFYING", "COPIED", "QUEUED", "COMPLETE" }[row % 5];
                graphics.DrawString((row + 1).ToString("D4"), bodyFont, palette[4], 28, y + 17);
                graphics.DrawString(state, titleFont, palette[5], 130, y + 14);
                graphics.DrawString("node-" + ((row * 17) % 97).ToString("D2") + " / batch-" + (row * 7919).ToString("x6"), bodyFont, palette[2], 340, y + 18);
                graphics.DrawString(((row * 37) % 9999).ToString("N0") + " ms", bodyFont, palette[1], ClientSize.Width - 180, y + 18);
            }
        }

        private void DrawVideo(Graphics graphics)
        {
            graphics.Clear(Color.FromArgb(11, 13, 16));
            const int cell = 46;
            int phase = tick % palette.Length;
            for (int y = 0; y < ClientSize.Height + cell; y += cell)
            {
                for (int x = 0; x < ClientSize.Width + cell; x += cell)
                {
                    int color = (x / cell + y / cell + phase) % palette.Length;
                    int shift = (tick * 7 + (y / cell) * 13) % cell;
                    graphics.FillRectangle(palette[color], x + shift - cell, y, cell - 2, cell - 2);
                }
            }
            int barWidth = Math.Max(160, ClientSize.Width / 5);
            int barX = (tick * 19) % (ClientSize.Width + barWidth) - barWidth;
            graphics.FillRectangle(palette[0], barX, 0, barWidth, ClientSize.Height);
            using (var outline = new Pen(Color.White, 6))
            {
                graphics.DrawRectangle(outline, barX + 12, 12, barWidth - 24, ClientSize.Height - 24);
            }
            graphics.DrawString(DateTime.UtcNow.ToString("O"), titleFont, palette[5], 32, ClientSize.Height - 58);
        }

        protected override void Dispose(bool disposing)
        {
            if (disposing)
            {
                timer.Dispose();
                titleFont.Dispose();
                bodyFont.Dispose();
                borderPen.Dispose();
                foreach (var brush in palette) brush.Dispose();
            }
            base.Dispose(disposing);
        }
    }
}
'@

function Focus-VisualWindow {
    if ($VisualProcessId -le 0) {
        return
    }
    $process = Get-Process -Id $VisualProcessId -ErrorAction SilentlyContinue
    if ($null -eq $process -or $process.MainWindowHandle -eq 0) {
        throw "Visual benchmark process $VisualProcessId is not available."
    }
    [ScreenShareBenchmarkFocus]::ShowWindowAsync($process.MainWindowHandle, 3) | Out-Null
    [ScreenShareBenchmarkFocus]::SwitchToThisWindow($process.MainWindowHandle, $true)
}

function Get-ObjectValue {
    param(
        [AllowNull()]$Object,
        [string]$Name,
        [AllowNull()]$Default = $null
    )

    if ($null -eq $Object) {
        return $Default
    }
    $property = $Object.PSObject.Properties[$Name]
    if ($null -eq $property -or $null -eq $property.Value) {
        return $Default
    }
    return $property.Value
}

function Get-Percentile {
    param(
        [double[]]$Values,
        [ValidateRange(1, 100)]
        [int]$Percentile
    )

    if ($null -eq $Values -or $Values.Count -eq 0) {
        return $null
    }
    $sorted = @($Values | Sort-Object)
    $rank = [Math]::Max(1, [Math]::Ceiling($sorted.Count * $Percentile / 100.0))
    return [double]$sorted[$rank - 1]
}

function Get-Average {
    param([double[]]$Values)

    if ($null -eq $Values -or $Values.Count -eq 0) {
        return $null
    }
    return [double](($Values | Measure-Object -Average).Average)
}

function New-ScreenShareHttpClient {
    $handler = [System.Net.Http.HttpClientHandler]::new()
    $handler.AllowAutoRedirect = $true
    $handler.UseCookies = $true
    $handler.CookieContainer = [System.Net.CookieContainer]::new()
    $handler.MaxConnectionsPerServer = 256
    $client = [System.Net.Http.HttpClient]::new($handler)
    $client.Timeout = [System.Threading.Timeout]::InfiniteTimeSpan

    if (-not [string]::IsNullOrEmpty($Password)) {
        $pairs = @()
        if (-not [string]::IsNullOrEmpty($Username)) {
            $pairs += "username=$([Uri]::EscapeDataString($Username))"
        }
        $pairs += "password=$([Uri]::EscapeDataString($Password))"
        $content = [System.Net.Http.StringContent]::new(
            ($pairs -join '&'),
            [Text.Encoding]::UTF8,
            'application/x-www-form-urlencoded'
        )
        try {
            $response = $client.PostAsync("$BaseUrl/auth", $content).GetAwaiter().GetResult()
            if (-not $response.IsSuccessStatusCode) {
                throw "Authentication failed with HTTP $([int]$response.StatusCode)"
            }
            $response.Dispose()
        }
        finally {
            $content.Dispose()
        }
    }

    return $client
}

function Open-BenchmarkViewer {
    param(
        [System.Net.Http.HttpClient]$Client,
        [System.Threading.CancellationToken]$CancellationToken,
        [bool]$Reconnect
    )

    $query = "benchmark=$([Guid]::NewGuid().ToString('N'))"
    if ($Reconnect) {
        $query += '&reconnect=1'
    }
    $response = $Client.GetAsync(
        "$BaseUrl/stream?$query",
        [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead,
        $CancellationToken
    ).GetAwaiter().GetResult()
    if (-not $response.IsSuccessStatusCode) {
        $statusCode = [int]$response.StatusCode
        $response.Dispose()
        throw "Viewer stream failed with HTTP $statusCode"
    }
    $stream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
    $pump = $stream.CopyToAsync(
        [System.IO.Stream]::Null,
        81920,
        $CancellationToken
    )
    return [pscustomobject]@{
        Response = $response
        Stream = $stream
        Pump = $pump
    }
}

function Close-BenchmarkViewer {
    param([AllowNull()]$Viewer)

    if ($null -eq $Viewer) {
        return
    }
    try { $Viewer.Stream.Dispose() } catch {}
    try { $Viewer.Response.Dispose() } catch {}
}

function Get-ScreenShareStatus {
    param([System.Net.Http.HttpClient]$Client)

    $json = $Client.GetStringAsync("$BaseUrl/status").GetAwaiter().GetResult()
    $status = $json | ConvertFrom-Json
    if (-not [bool](Get-ObjectValue $status 'active' $false)) {
        throw 'Screen sharing is not active.'
    }
    if ($null -eq (Get-ObjectValue $status 'media_metrics' $null)) {
        throw 'The running screen-share server does not expose media_metrics. Rebuild and restart the app first.'
    }
    return $status
}

function Resolve-BenchmarkProcess {
    if ($ProcessId -gt 0) {
        return Get-Process -Id $ProcessId -ErrorAction Stop
    }

    $candidate = Get-Process -ErrorAction SilentlyContinue |
        Where-Object { $_.ProcessName -match '^(file-sync-tool.*|app)$' } |
        Sort-Object StartTime -Descending |
        Select-Object -First 1
    return $candidate
}

function Get-ProcessSample {
    param(
        [AllowNull()]$Process,
        [AllowNull()][Nullable[double]]$PreviousCpuMs,
        [AllowNull()][Nullable[datetime]]$PreviousAt
    )

    if ($null -eq $Process) {
        return [pscustomobject]@{
            CpuPercent = $null
            WorkingSetMb = $null
            CpuMs = $null
            SampledAt = [DateTime]::UtcNow
        }
    }

    try {
        $Process.Refresh()
        $sampledAt = [DateTime]::UtcNow
        $cpuMs = $Process.TotalProcessorTime.TotalMilliseconds
        $cpuPercent = $null
        if ($null -ne $PreviousCpuMs -and $null -ne $PreviousAt) {
            $elapsedMs = ($sampledAt - $PreviousAt).TotalMilliseconds
            if ($elapsedMs -gt 0) {
                $cpuPercent = 100.0 * ($cpuMs - $PreviousCpuMs) /
                    ($elapsedMs * [Environment]::ProcessorCount)
            }
        }
        return [pscustomobject]@{
            CpuPercent = $cpuPercent
            WorkingSetMb = $Process.WorkingSet64 / 1MB
            CpuMs = $cpuMs
            SampledAt = $sampledAt
        }
    }
    catch {
        return [pscustomobject]@{
            CpuPercent = $null
            WorkingSetMb = $null
            CpuMs = $null
            SampledAt = [DateTime]::UtcNow
        }
    }
}

$BaseUrl = $BaseUrl.TrimEnd('/')
$resolvedOutputDirectory = [IO.Path]::GetFullPath((Join-Path (Get-Location) $OutputDirectory))
[IO.Directory]::CreateDirectory($resolvedOutputDirectory) | Out-Null
$timestamp = [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss')
$csvPath = Join-Path $resolvedOutputDirectory "mjpeg-$timestamp.csv"
$jsonPath = Join-Path $resolvedOutputDirectory "mjpeg-$timestamp-summary.json"

$rows = [Collections.Generic.List[object]]::new()
$summaries = [Collections.Generic.List[object]]::new()
$statusClient = New-ScreenShareHttpClient
$trackedProcess = Resolve-BenchmarkProcess

try {
    foreach ($scenarioName in $Scenario) {
        [ScreenShareSyntheticVisual]::Stop()
        if ($SyntheticVisual -and $scenarioName -ne 'static') {
            [ScreenShareSyntheticVisual]::Start($scenarioName)
        }
        if (-not $NonInteractive) {
            Write-Host ''
            Write-Host "Prepare the shared desktop for scenario '$scenarioName'." -ForegroundColor Cyan
            [void](Read-Host 'Press Enter when ready')
        }

        foreach ($requestedViewerCount in $ViewerCount) {
            Write-Host "Running $scenarioName with $requestedViewerCount viewer(s)..." -ForegroundColor Cyan
            Focus-VisualWindow
            $before = Get-ScreenShareStatus $statusClient
            $beforeMedia = Get-ObjectValue $before 'media_metrics' $null
            $beforeDropped = [uint64](Get-ObjectValue $beforeMedia 'slow_client_dropped_frames' 0)
            $beforeReconnects = [uint64](Get-ObjectValue $beforeMedia 'stream_reconnect_count' 0)

            $cancellation = [Threading.CancellationTokenSource]::new()
            $viewerClients = [Collections.Generic.List[System.Net.Http.HttpClient]]::new()
            $viewers = [Collections.Generic.List[object]]::new()

            try {
                for ($index = 0; $index -lt $requestedViewerCount; $index++) {
                    $client = New-ScreenShareHttpClient
                    $viewerClients.Add($client)
                    $viewers.Add((Open-BenchmarkViewer $client $cancellation.Token $false))
                }

                if ($WarmupSeconds -gt 0) {
                    Focus-VisualWindow
                    Start-Sleep -Seconds $WarmupSeconds
                }

                $startedAt = [DateTime]::UtcNow
                $reconnectDone = $false
                $processSample = Get-ProcessSample $trackedProcess $null $null
                $previousCpuMs = $processSample.CpuMs
                $previousCpuAt = $processSample.SampledAt

                while (([DateTime]::UtcNow - $startedAt).TotalSeconds -lt $DurationSeconds) {
                    Focus-VisualWindow
                    Start-Sleep -Milliseconds $SampleIntervalMs
                    $elapsedSeconds = ([DateTime]::UtcNow - $startedAt).TotalSeconds

                    if (-not $reconnectDone -and $elapsedSeconds -ge ($DurationSeconds / 2.0)) {
                        Close-BenchmarkViewer $viewers[0]
                        $viewers[0] = Open-BenchmarkViewer $viewerClients[0] $cancellation.Token $true
                        $reconnectDone = $true
                    }

                    $status = Get-ScreenShareStatus $statusClient
                    $media = Get-ObjectValue $status 'media_metrics' $null
                    $processSample = Get-ProcessSample $trackedProcess $previousCpuMs $previousCpuAt
                    $previousCpuMs = $processSample.CpuMs
                    $previousCpuAt = $processSample.SampledAt

                    $rows.Add([pscustomobject]@{
                        sampled_at_utc = [DateTime]::UtcNow.ToString('o')
                        scenario = $scenarioName
                        requested_viewers = $requestedViewerCount
                        elapsed_seconds = [Math]::Round($elapsedSeconds, 3)
                        active_viewers = [int](Get-ObjectValue $status 'viewers' 0)
                        fps_actual = [double](Get-ObjectValue $status 'fps_actual' 0)
                        frame_age_ms = Get-ObjectValue $media 'frame_age_ms' $null
                        outbound_mbps = [Math]::Round(([double](Get-ObjectValue $status 'bitrate_kbps' 0) / 1024.0), 3)
                        jpeg_avg_bytes = [uint64](Get-ObjectValue $media 'jpeg_size_avg_bytes' 0)
                        jpeg_p50_bytes = [uint64](Get-ObjectValue $media 'jpeg_size_p50_bytes' 0)
                        jpeg_p95_bytes = [uint64](Get-ObjectValue $media 'jpeg_size_p95_bytes' 0)
                        first_frame_delay_ms = Get-ObjectValue $media 'first_frame_delay_ms' $null
                        stream_first_frame_p95_ms = Get-ObjectValue $media 'stream_first_frame_p95_ms' $null
                        reconnect_p95_ms = Get-ObjectValue $media 'stream_reconnect_p95_ms' $null
                        dropped_frames_total = [uint64](Get-ObjectValue $media 'slow_client_dropped_frames' 0)
                        process_cpu_percent = if ($null -eq $processSample.CpuPercent) { $null } else { [Math]::Round($processSample.CpuPercent, 2) }
                        process_working_set_mb = if ($null -eq $processSample.WorkingSetMb) { $null } else { [Math]::Round($processSample.WorkingSetMb, 2) }
                    })
                }

                $after = Get-ScreenShareStatus $statusClient
                $afterMedia = Get-ObjectValue $after 'media_metrics' $null
                $scenarioRows = @($rows | Where-Object {
                    $_.scenario -eq $scenarioName -and $_.requested_viewers -eq $requestedViewerCount
                })
                $fpsValues = @($scenarioRows | ForEach-Object { [double]$_.fps_actual })
                $ageValues = @($scenarioRows | Where-Object { $null -ne $_.frame_age_ms } | ForEach-Object { [double]$_.frame_age_ms })
                $bitrateValues = @($scenarioRows | ForEach-Object { [double]$_.outbound_mbps })
                $cpuValues = @($scenarioRows | Where-Object { $null -ne $_.process_cpu_percent } | ForEach-Object { [double]$_.process_cpu_percent })
                $memoryValues = @($scenarioRows | Where-Object { $null -ne $_.process_working_set_mb } | ForEach-Object { [double]$_.process_working_set_mb })

                $summaries.Add([pscustomobject]@{
                    scenario = $scenarioName
                    requested_viewers = $requestedViewerCount
                    samples = $scenarioRows.Count
                    fps_average = Get-Average $fpsValues
                    fps_minimum = if ($fpsValues.Count) { ($fpsValues | Measure-Object -Minimum).Minimum } else { $null }
                    frame_age_p95_ms = Get-Percentile $ageValues 95
                    outbound_average_mbps = Get-Average $bitrateValues
                    outbound_peak_mbps = if ($bitrateValues.Count) { ($bitrateValues | Measure-Object -Maximum).Maximum } else { $null }
                    cpu_average_percent = Get-Average $cpuValues
                    cpu_peak_percent = if ($cpuValues.Count) { ($cpuValues | Measure-Object -Maximum).Maximum } else { $null }
                    working_set_average_mb = Get-Average $memoryValues
                    working_set_peak_mb = if ($memoryValues.Count) { ($memoryValues | Measure-Object -Maximum).Maximum } else { $null }
                    jpeg_average_bytes = [uint64](Get-ObjectValue $afterMedia 'jpeg_size_avg_bytes' 0)
                    jpeg_p50_bytes = [uint64](Get-ObjectValue $afterMedia 'jpeg_size_p50_bytes' 0)
                    jpeg_p95_bytes = [uint64](Get-ObjectValue $afterMedia 'jpeg_size_p95_bytes' 0)
                    capture_first_frame_ms = Get-ObjectValue $afterMedia 'first_frame_delay_ms' $null
                    stream_first_frame_p95_ms = Get-ObjectValue $afterMedia 'stream_first_frame_p95_ms' $null
                    reconnect_p95_ms = Get-ObjectValue $afterMedia 'stream_reconnect_p95_ms' $null
                    reconnects_observed = [uint64](Get-ObjectValue $afterMedia 'stream_reconnect_count' 0) - $beforeReconnects
                    dropped_frames = [uint64](Get-ObjectValue $afterMedia 'slow_client_dropped_frames' 0) - $beforeDropped
                })
            }
            finally {
                $cancellation.Cancel()
                foreach ($viewer in $viewers) {
                    Close-BenchmarkViewer $viewer
                }
                foreach ($client in $viewerClients) {
                    $client.Dispose()
                }
                $cancellation.Dispose()
            }
        }
        [ScreenShareSyntheticVisual]::Stop()
    }
}
finally {
    [ScreenShareSyntheticVisual]::Stop()
    $statusClient.Dispose()
}

$rows | Export-Csv -LiteralPath $csvPath -NoTypeInformation -Encoding UTF8
$report = [pscustomobject]@{
    schema_version = 1
    generated_at_utc = [DateTime]::UtcNow.ToString('o')
    base_url = $BaseUrl
    duration_seconds = $DurationSeconds
    warmup_seconds = $WarmupSeconds
    sample_interval_ms = $SampleIntervalMs
    process_id = if ($null -eq $trackedProcess) { $null } else { $trackedProcess.Id }
    visual_process_id = if ($VisualProcessId -gt 0) { $VisualProcessId } else { $null }
    synthetic_visual = [bool]$SyntheticVisual
    scenarios = @($summaries)
}
$report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $jsonPath -Encoding UTF8

[pscustomobject]@{
    CsvPath = $csvPath
    SummaryPath = $jsonPath
    ScenarioCount = $summaries.Count
}
