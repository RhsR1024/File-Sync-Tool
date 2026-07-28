/**
 * Test-only fixtures shared by the field-evidence and full-spec audit tests.
 *
 * These are the minimal *shapes* a real field session must produce; the numbers
 * are placeholders. Nothing here may be copied into an artifacts directory and
 * presented as a measurement.
 */

export function distributionFixture(overrides = {}) {
  return {
    p50: 40,
    p95: 80,
    p99: 120,
    sample_count: 1800,
    retained_sample_count: 512,
    capacity: 512,
    measurement_scope: 'rolling-window',
    ...overrides,
  };
}

function performanceRun(id, overrides) {
  return {
    id,
    cpu_generation: 'broadwell',
    resolution_tier: '720p30',
    fps: 30,
    scenario: 'static',
    capture_backend: 'wgc',
    transport: 'mse_h264',
    healthy_client_count: 1,
    duration_minutes: 30,
    presentation_trace_source: 'expected-display-time',
    distributions: {
      capture_to_display_ms: distributionFixture(),
      input_to_sendinput_ms: distributionFixture({ p50: 5, p95: 12, p99: 30 }),
      input_to_visible_response_ms: distributionFixture({ p50: 60, p95: 110, p99: 180 }),
      live_edge_distance_ms: distributionFixture({ p50: 120, p95: 160, p99: 220 }),
      outbound_bitrate_100ms_bps: distributionFixture({ p50: 3_000_000, p95: 5_000_000, p99: 7_000_000 }),
      outbound_bitrate_1s_bps: distributionFixture({ p50: 3_200_000, p95: 4_800_000, p99: 6_400_000 }),
      idr_size_bytes: distributionFixture({ p50: 40_000, p95: 60_000, p99: 90_000 }),
      fanout_send_ms: distributionFixture({ p50: 1, p95: 6, p99: 18 }),
      input_queue_age_ms: distributionFixture({ p50: 2, p95: 8, p99: 20 }),
      reconnect_recovery_ms: distributionFixture({ p50: 300, p95: 700, p99: 1200 }),
    },
    frame_accounting: { presented_frames: 54_000, dropped_frames: 120, dropped_ratio: 0.0022 },
    input_queue: { depth_max: 12, coalesced_count: 430, full_count: 0 },
    resource_usage: {
      host_cpu_percent: 18.5,
      host_gpu_percent: 22.4,
      host_memory_mb: 310,
      viewer_cpu_percent: 12.1,
      viewer_memory_mb: 260,
    },
    ...overrides,
  };
}

export function performanceMatrixReport(overrides = {}) {
  return {
    scope: 'performance-matrix',
    status: 'passed',
    run_id: 'perf-matrix-2026-07-28',
    runs: [
      performanceRun('broadwell-wgc-static-720p30'),
      performanceRun('skylake-dxgi-dynamic-1080p30', {
        cpu_generation: 'skylake',
        resolution_tier: '1080p30',
        scenario: 'dynamic',
        capture_backend: 'dxgi',
        healthy_client_count: 5,
      }),
      performanceRun('intel10-rdp-video-720p60', {
        cpu_generation: 'intel-10th',
        scenario: 'video',
        capture_backend: 'rdp',
        fps: 60,
        healthy_client_count: 20,
      }),
      performanceRun('intel10-basic-fastscroll-1080p60', {
        cpu_generation: 'intel-10th',
        resolution_tier: '1080p30',
        scenario: 'fast-scroll',
        capture_backend: 'basic-display-adapter',
        fps: 60,
        healthy_client_count: 30,
        transport: 'mjpeg',
      }),
    ],
    ...overrides,
  };
}

export function independentViewingDevicesReport(overrides = {}) {
  const devices = Array.from({ length: 22 }, (_, index) => ({
    id: `viewer-${String(index + 1).padStart(2, '0')}`,
    os: 'Windows 10 22H2',
    browser: index % 2 === 0 ? 'chrome' : 'edge',
    browser_version: '150.0.0.0',
    network_segment: '192.168.30.0/24',
    independent_hardware: true,
  }));
  return {
    scope: 'independent-viewing-devices',
    status: 'passed',
    session_id: 'independent-devices-2026-07-28',
    devices,
    tab_client_substitution: { tab_client_count: 0, used_as_device_substitute: false },
    fanout_session: {
      duration_minutes: 30,
      peak_concurrent_viewers: 30,
      healthy_lagged_frames: 0,
      state_reclaim_seconds: 1.4,
    },
    ...overrides,
  };
}

export function managedBrowserExternalMediaReport(overrides = {}) {
  return {
    scope: 'managed-browser-external-media',
    status: 'passed',
    session_id: 'managed-browser-2026-07-28',
    managed_browser_external_acceptance: true,
    synthetic_loopback_only: false,
    browsers: [
      { name: 'edge', version: '150.0.0.0', managed: true, policy_scope: 'machine' },
      { name: 'chrome', version: '150.0.0.0', managed: true, policy_scope: 'user' },
    ],
    transports: ['web_rtc', 'web_codecs'],
    real_media_playback: { verified: true, transport: 'web_rtc', rendered_frames: 5400 },
    external_peer: { independent_host: true, network_segment: '192.168.30.0/24' },
    secure_context: {
      https_terminated: true,
      certificate_trusted: true,
      certificate_rotation_tested: true,
      browser_profile_clear_tested: true,
      dhcp_ip_change_tested: true,
    },
    ...overrides,
  };
}

export function networkImpairmentRecoveryReport(overrides = {}) {
  return {
    scope: 'network-impairment-recovery',
    status: 'passed',
    session_id: 'impairment-2026-07-28',
    thresholds: { maximum_recovery_p99_ms: 3000, maximum_frame_gap_ms: 1500 },
    injections: [
      {
        kind: 'loss',
        magnitude: '5% random loss',
        tool: 'clumsy 0.3',
        recovery_ms: distributionFixture({ p50: 400, p95: 900, p99: 1600 }),
        frame_continuity: { recovered: true, max_gap_ms: 900 },
      },
      {
        kind: 'jitter',
        magnitude: '±60 ms',
        tool: 'clumsy 0.3',
        recovery_ms: distributionFixture({ p50: 250, p95: 700, p99: 1200 }),
        frame_continuity: { recovered: true, max_gap_ms: 640 },
      },
    ],
    ...overrides,
  };
}

function transportCandidate(transport, overrides = {}) {
  return {
    transport,
    capture_to_display_ms: distributionFixture(),
    input_to_visible_response_ms: distributionFixture({ p50: 70, p95: 130, p99: 190 }),
    recovery_after_impairment_ms: distributionFixture({ p50: 300, p95: 800, p99: 1400 }),
    outbound_bitrate_bps: 4_200_000,
    host_cpu_percent: 19.4,
    per_viewer_memory_mb: 14.2,
    join_leave_idr_count: 36,
    ...overrides,
  };
}

export function transportSelectionReport(overrides = {}) {
  return {
    scope: 'transport-selection',
    status: 'passed',
    run_id: 'transport-selection-2026-07-28',
    candidates: [
      transportCandidate('mse_h264'),
      transportCandidate('web_codecs'),
      transportCandidate('web_rtc'),
    ],
    comparison_conditions: {
      same_conditions: true,
      client_count: 30,
      cpu_generations: ['broadwell', 'skylake', 'intel-10th'],
    },
    decision: { selected: 'mse_h264', rationale: 'No candidate was clearly better under identical conditions.' },
    fps_default_decision: {
      selected_fps: 30,
      rationale: '60 FPS raised host GPU load without improving capture-to-display P99 on Broadwell.',
      evidence_run_ids: ['intel10-rdp-video-720p60', 'broadwell-wgc-static-720p30'],
    },
    ...overrides,
  };
}

export function featureRegressionReport(overrides = {}) {
  const names = [
    'annotations',
    'control_request_grant',
    'control_request_revoke',
    'keyboard_mouse_release_all',
    'cursor',
    'multi_monitor_switch',
    'privacy_black_screen_recovery',
    'wgc_backend',
    'dxgi_backend',
    'rdp_session',
    'software_encoder_fallback',
    'mjpeg_fallback',
  ];
  return {
    scope: 'feature-regression',
    status: 'passed',
    run_id: 'feature-regression-2026-07-28',
    checks: Object.fromEntries(names.map((name) => [name, { tested: true, passed: true }])),
    localization: { zh_cn: true, en_us: true },
    ...overrides,
  };
}

export const FIELD_EVIDENCE_FIXTURES = {
  performance_matrix: performanceMatrixReport,
  independent_viewing_devices: independentViewingDevicesReport,
  managed_browser_external_media: managedBrowserExternalMediaReport,
  network_impairment_recovery: networkImpairmentRecoveryReport,
  transport_selection: transportSelectionReport,
  feature_regression: featureRegressionReport,
};
