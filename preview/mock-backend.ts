/**
 * A stand-in for the Tauri command bridge so the real device simulator page can
 * be reviewed in a browser. Everything here is fixture data — it exists only to
 * exercise the UI states that are otherwise reachable only on a machine with the
 * asset packs installed and a live UMS platform to talk to.
 *
 * Install it before the app module loads: `invoke` resolves through
 * `window.__TAURI_INTERNALS__` at call time, so replacing that object is enough
 * to intercept every command and event subscription.
 */

export type PreviewScenario = 'clear' | 'warning' | 'blocked' | 'running';

interface Internals {
  metadata: { currentWindow: { label: string }; currentWebview: { label: string; windowLabel: string } };
  transformCallback(callback: (payload: unknown) => void, once?: boolean): number;
  unregisterCallback(id: number): void;
  invoke(command: string, args?: Record<string, unknown>): Promise<unknown>;
}

const SCENARIO_IDS: PreviewScenario[] = ['clear', 'warning', 'blocked', 'running'];
const requested = new URLSearchParams(window.location.search).get('s') as PreviewScenario | null;
/** `?s=blocked` opens straight into a state, which also makes it linkable. */
let scenario: PreviewScenario = requested && SCENARIO_IDS.includes(requested) ? requested : 'warning';

export function setScenario(next: PreviewScenario) {
  scenario = next;
}

export function currentScenario() {
  return scenario;
}

const NETWORK_INTERFACES = [
  {
    id: 'guid:8b1f4a20-77c3-4f0e-9a1d-2c5e7b904311',
    name: '以太网',
    description: 'Intel(R) Ethernet Connection I219-LM',
    is_enabled: true,
    is_up: true,
    // CIDR form: adapter matching parses these as `address/prefix`.
    ipv4_addresses: ['192.168.1.42/24'],
  },
  {
    id: 'guid:0d3a9f71-5be2-4c88-b1f4-6a0d2e8c5527',
    name: 'WLAN',
    description: 'Intel(R) Wi-Fi 6 AX201 160MHz',
    is_enabled: true,
    is_up: true,
    ipv4_addresses: ['10.20.30.15/24'],
  },
];

const PROFILE_IDS = [
  'ipc-custom',
  'ipc-smart',
  'ipc-structured',
  'ipc-face-access',
  'nvr-common',
  'nvr-vehicle',
];

const PROFILES = PROFILE_IDS.map((id) => ({
  id,
  display_name_key: `deviceSimulator.profiles.${id}`,
  device_kind: id.startsWith('nvr-') ? 'nvr' : 'ipc',
  supported_platforms: ['ums'],
  availability: 'local',
  installed_version: '1.4.0',
  available_version: '1.4.0',
  verified_platforms: [],
}));

const MEDIA_THEMES = [
  { id: 'classic', display_name_key: 'deviceSimulator.mediaThemes.classic', is_default: true },
  { id: 'windows-tech', display_name_key: 'deviceSimulator.mediaThemes.windowsTech', is_default: false },
  { id: 'fanren-xiuxian', display_name_key: 'deviceSimulator.mediaThemes.fanrenXiuxian', is_default: false },
  { id: 'green-hill-running', display_name_key: 'deviceSimulator.mediaThemes.greenHillRunning', is_default: false },
];

const ALARM_TYPES = [
  {
    profile_id: 'ipc-custom',
    alarm_types: [
      { id: 'motion', display_name: '移动侦测', supports_pictures: true },
      { id: 'line-cross', display_name: '越界侦测', supports_pictures: true },
      { id: 'region-enter', display_name: '区域入侵', supports_pictures: true },
      { id: 'tamper', display_name: '视频遮挡', supports_pictures: false },
    ],
  },
  {
    profile_id: 'nvr-common',
    alarm_types: [
      { id: 'disk-full', display_name: '磁盘已满', supports_pictures: false },
      { id: 'channel-offline', display_name: '通道离线', supports_pictures: false },
    ],
  },
];

const SETTINGS = {
  asset_server_url_override: null,
  selected_interface_id: NETWORK_INTERFACES[0].id,
  last_platform: 'ums',
  last_start_ip: '192.168.1.100',
  last_device_ips: [],
  last_subnet_prefix: 24,
  last_platform_servers: [{ id: 'server-ums-1', host: '192.168.1.8', port: 80 }],
  last_platform_access_mode: 'open',
  last_alarm_receiver_url: null,
  last_alarm_receiver_port: 22_815,
  last_device_groups: [
    { id: 'group-ipc', profile_id: 'ipc-custom', count: 8, nvr_channel_count: null },
    { id: 'group-nvr', profile_id: 'nvr-common', count: 2, nvr_channel_count: 16 },
  ],
  last_http_port: 81,
  last_rtsp_ports: { main: 554, sub: 555, third: 556 },
  last_media_theme_id: 'classic',
  auto_check_asset_updates: true,
  manage_firewall: true,
};

const ASSET_STATUS = {
  state: 'ready',
  profile_ids: ['ipc-custom', 'nvr-common'],
  packs: [
    { id: 'protocol-core', required_version: '1.4.0', installed_version: '1.4.0', size: 18_452_112, state: 'ready', error_code: null },
    { id: 'media-classic', required_version: '1.2.0', installed_version: '1.2.0', size: 96_314_880, state: 'ready', error_code: null },
    { id: 'ipc-custom', required_version: '1.4.0', installed_version: '1.4.0', size: 4_210_688, state: 'ready', error_code: null },
    { id: 'nvr-common', required_version: '1.4.0', installed_version: '1.4.0', size: 5_872_640, state: 'ready', error_code: null },
  ],
  update_available: false,
  error_code: null,
};

function ipAt(offset: number) {
  return `192.168.1.${100 + offset}`;
}

function macAt(offset: number) {
  return `00:1B:4F:${(0x20 + offset).toString(16).toUpperCase().padStart(2, '0')}:A9:${offset
    .toString(16)
    .toUpperCase()
    .padStart(2, '0')}`;
}

function buildPreview(request: Record<string, unknown> | undefined) {
  const groups = (request?.groups as Array<Record<string, unknown>> | undefined) ?? SETTINGS.last_device_groups;
  const explicit = (request?.device_ips as string[] | undefined) ?? [];
  const httpPortless = { main: 554, sub: 555, third: 556 };
  const ports = (request?.rtsp_ports as typeof httpPortless | undefined) ?? httpPortless;
  const devices: unknown[] = [];
  let offset = 0;
  let totalChannels = 0;

  for (const group of groups) {
    const profileId = String(group.profile_id ?? 'ipc-custom');
    const count = Math.max(0, Number(group.count) || 0);
    const isNvr = profileId.startsWith('nvr-');
    const channelCount = isNvr ? Math.max(1, Number(group.nvr_channel_count) || 1) : null;
    for (let index = 0; index < count; index += 1) {
      const ip = explicit[offset] ?? ipAt(offset);
      const deviceId = `${group.id ?? 'group'}-${String(index + 1).padStart(4, '0')}`;
      totalChannels += channelCount ?? 1;
      devices.push({
        device_id: deviceId,
        group_id: String(group.id ?? 'group'),
        profile_id: profileId,
        device_kind: isNvr ? 'nvr' : 'ipc',
        ip,
        mac: macAt(offset),
        serial_number: `FST${String(offset + 1).padStart(10, '0')}`,
        hardware_id: `HW-${profileId.toUpperCase()}-${offset + 1}`,
        channel_count: channelCount,
        streams: (['main', 'sub', 'third'] as const).map((stream) => ({
          device_id: deviceId,
          channel_id: isNvr ? 'ch01' : null,
          stream,
          url: `rtsp://${ip}:${ports[stream]}/${stream}`,
        })),
      });
      offset += 1;
    }
  }

  return { total_devices: devices.length, total_channels: totalChannels, devices };
}

function check(id: string, status: 'passed' | 'warning' | 'failed', details: string | null = null) {
  const severity = status === 'failed' ? 'error' : status === 'warning' ? 'warning' : 'info';
  const keys: Record<string, string> = {
    request: 'request',
    assets: 'assets',
    'profile-evidence': 'profileEvidence',
    recovery: 'recovery',
    interface: 'interface',
    'local-addresses': 'localAddresses',
    'address-conflicts': 'addressConflicts',
    ports: 'ports',
    'platform-config': 'platformConfig',
    'platform-connectivity': 'platformConnectivity',
    worker: 'worker',
    firewall: 'firewall',
  };
  return {
    id,
    severity,
    status,
    message_key: `deviceSimulator.preflight.checks.${keys[id]}`,
    details,
  };
}

function buildPreflight(request: Record<string, unknown> | undefined) {
  const devicePreview = buildPreview(request);
  const addresses = (devicePreview.devices as Array<{ ip: string }>).map((device) => device.ip);

  const clearAssessment = (address: string) => ({
    address,
    verdict: 'clear',
    strongest_evidence: 'probe',
    evidence: [
      { address, kind: 'probe', result: 'available', details: 'no ARP reply after 2 attempts' },
    ],
  });
  const conflictAssessment = (address: string, mac: string) => ({
    address,
    verdict: 'conflict',
    strongest_evidence: 'probe',
    evidence: [{ address, kind: 'probe', result: 'occupied', details: mac }],
  });
  const unknownAssessment = (address: string) => ({
    address,
    verdict: 'unknown',
    strongest_evidence: 'probe',
    evidence: [
      {
        address,
        kind: 'probe',
        result: 'inconclusive',
        details: 'address is not on-link for any local interface, so ARP cannot reach it',
      },
    ],
  });

  const base = [
    check('request', 'passed', `${devicePreview.total_devices} devices / ${devicePreview.total_channels} channels`),
    check('assets', 'passed', '4 signed pack(s); catalog 2026-07-19T03:30:00+08:00'),
    // Always a warning in practice: this is exactly the advisory the page now hides.
    check('profile-evidence', 'warning', 'static legacy evidence was reviewed and approved for local execution'),
    check('recovery', 'passed'),
    check('interface', 'passed', '以太网 (Intel(R) Ethernet Connection I219-LM)'),
    check('local-addresses', 'passed'),
    check('ports', 'passed'),
    check('platform-config', 'passed'),
    check('worker', 'passed'),
    check('firewall', 'passed'),
  ];

  if (scenario === 'blocked') {
    return {
      ok: false,
      checks: [
        ...base,
        check('address-conflicts', 'failed', `conflict evidence: ${addresses[2]}, ${addresses[5]}`),
        check('platform-connectivity', 'warning', 'bounded connection probe failed: ["server-ums-1"]'),
      ],
      device_preview: devicePreview,
      address_assessments: addresses.map((address, index) => (index === 2
        ? conflictAssessment(address, '00:0C:29:7A:1E:44')
        : index === 5
          ? conflictAssessment(address, 'B8:27:EB:11:9C:03')
          : clearAssessment(address))),
    };
  }

  if (scenario === 'warning') {
    return {
      ok: true,
      checks: [
        ...base,
        check('address-conflicts', 'warning', 'one or more addresses remain inconclusive'),
        check('platform-connectivity', 'warning', 'connectivity has not been verified in the current environment'),
      ],
      device_preview: devicePreview,
      address_assessments: addresses.map((address, index) => (index >= addresses.length - 2
        ? unknownAssessment(address)
        : clearAssessment(address))),
    };
  }

  return {
    ok: true,
    checks: [...base, check('address-conflicts', 'passed'), check('platform-connectivity', 'passed')],
    device_preview: devicePreview,
    address_assessments: addresses.map(clearAssessment),
  };
}

function runningStatus() {
  return {
    state: 'running',
    session_id: 'session-preview-0001',
    started_at: Date.now() - 184_000,
    phase_progress: null,
    metrics: {
      total_devices: 10,
      online_devices: 10,
      total_channels: 40,
      active_rtsp_clients: 3,
      outbound_bitrate_kbps: 12_480,
      active_alarm_jobs: 0,
    },
    cleanup_stage: null,
    recovery_session_id: null,
    last_error: null,
  };
}

function idleStatus() {
  return {
    state: 'idle',
    session_id: null,
    started_at: null,
    phase_progress: null,
    metrics: {
      total_devices: 0,
      online_devices: 0,
      total_channels: 0,
      active_rtsp_clients: 0,
      outbound_bitrate_kbps: 0,
      active_alarm_jobs: 0,
    },
    cleanup_stage: null,
    recovery_session_id: null,
    last_error: null,
  };
}

/** Delivers an event to every listener the app registered for that name. */
const listeners = new Map<string, Set<number>>();
const callbacks = new Map<number, (payload: unknown) => void>();
let nextCallbackId = 0;

export function emitPreviewEvent(event: string, payload: unknown) {
  for (const id of listeners.get(event) ?? []) {
    callbacks.get(id)?.({ event, id, payload });
  }
}

const HANDLERS: Record<string, (args: Record<string, unknown> | undefined) => unknown> = {
  device_simulator_get_settings: () => SETTINGS,
  device_simulator_save_settings: (args) => args?.settings ?? SETTINGS,
  device_simulator_list_interfaces: () => NETWORK_INTERFACES,
  device_simulator_list_profiles: () => PROFILES,
  device_simulator_list_alarm_types: () => ALARM_TYPES,
  device_simulator_list_media_themes: () => MEDIA_THEMES,
  device_simulator_get_asset_status: () => ASSET_STATUS,
  device_simulator_preview_devices: (args) => buildPreview(args?.request as Record<string, unknown>),
  device_simulator_preflight: (args) => buildPreflight(args?.request as Record<string, unknown>),
  device_simulator_get_status: () => (scenario === 'running' ? runningStatus() : idleStatus()),
  device_simulator_start: () => {
    scenario = 'running';
    return runningStatus();
  },
  device_simulator_stop: () => {
    scenario = 'clear';
    return null;
  },
  device_simulator_prepare_assets: () => 'asset-job-preview',
  device_simulator_cancel_asset_download: () => null,
  device_simulator_import_alarm_image: () => ({
    image_id: 'image-preview',
    file_name: 'alarm-sample.jpg',
    size: 248_320,
  }),
  device_simulator_start_alarm: () => 'alarm-job-preview',
  device_simulator_trigger_alarm_once: () => ({
    attempted: 10, succeeded: 10, failed: 0, unverified: 0, in_flight: 0, errors: [],
  }),
  device_simulator_stop_alarm: () => null,
  device_simulator_recover: () => ({ recovered: true, error: null }),
  save_kv: () => null,
  load_kv: () => null,
};

export function installMockBackend() {
  const internals: Internals = {
    metadata: {
      currentWindow: { label: 'main' },
      currentWebview: { label: 'main', windowLabel: 'main' },
    },
    transformCallback(callback) {
      const id = (nextCallbackId += 1);
      callbacks.set(id, callback);
      return id;
    },
    unregisterCallback(id) {
      callbacks.delete(id);
    },
    async invoke(command, args) {
      if (command === 'plugin:event|listen') {
        const event = String(args?.event);
        const handler = Number(args?.handler);
        if (!listeners.has(event)) listeners.set(event, new Set());
        listeners.get(event)!.add(handler);
        return handler;
      }
      if (command === 'plugin:event|unlisten') {
        listeners.get(String(args?.event))?.delete(Number(args?.eventId));
        return null;
      }
      const handler = HANDLERS[command];
      if (!handler) {
        // Surfacing this beats a silent hang while reviewing the page.
        console.warn(`[preview] no fixture for command "${command}"`);
        return null;
      }
      // A touch of latency so spinners and disabled states are actually visible.
      await new Promise((resolve) => setTimeout(resolve, command === 'device_simulator_preflight' ? 600 : 60));
      return handler(args);
    },
  };
  (window as unknown as { __TAURI_INTERNALS__: Internals }).__TAURI_INTERNALS__ = internals;
}
