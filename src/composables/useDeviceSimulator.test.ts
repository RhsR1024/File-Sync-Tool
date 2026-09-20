import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { effectScope, type EffectScope } from 'vue';

const native = vi.hoisted(() => ({
  listen: vi.fn(),
  api: {
    getSettings: vi.fn(), listInterfaces: vi.fn(), listProfiles: vi.fn(),
    getStatus: vi.fn(), getLocalMaterialsPath: vi.fn(), previewDevices: vi.fn(),
    getAssetStatus: vi.fn(), listAlarmTypes: vi.fn(), listMediaThemes: vi.fn(),
  },
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen }));
vi.mock('@/lib/deviceSimulator', async (original) => ({
  ...await original<typeof import('@/lib/deviceSimulator')>(), deviceSimulatorApi: native.api,
}));

type Unlisten = () => void;
let pending: { resolve: (value: Unlisten) => void; reject: (error: Error) => void }[];
let simulator: ReturnType<typeof import('./useDeviceSimulator')['useDeviceSimulator']>;
let scope: EffectScope;

beforeEach(async () => {
  vi.resetModules();
  vi.resetAllMocks();
  pending = [];
  native.listen.mockImplementation(() => new Promise<Unlisten>((resolve, reject) => pending.push({ resolve, reject })));
  const { useDeviceSimulator } = await import('./useDeviceSimulator');
  scope = effectScope();
  simulator = scope.run(useDeviceSimulator)!;
  native.api.getSettings.mockImplementation(async () => JSON.parse(JSON.stringify(simulator.settings.value)));
  native.api.listInterfaces.mockResolvedValue([]);
  native.api.listProfiles.mockResolvedValue([]);
  native.api.getStatus.mockImplementation(async () => JSON.parse(JSON.stringify(simulator.status.value)));
  native.api.getLocalMaterialsPath.mockResolvedValue('materials');
  native.api.previewDevices.mockResolvedValue({ devices: [], total_devices: 0, total_channels: 0 });
  native.api.getAssetStatus.mockResolvedValue({ state: 'missing' });
});
afterEach(() => {
  simulator.dispose();
  scope.stop();
});

describe('simulator event lifecycle', () => {
  it.each(['previewDevices', 'getAssetStatus'] as const)('ignores a disposed initialization waiting for %s', async (method) => {
    native.listen.mockImplementation(async () => vi.fn());
    let resolveOld!: (value: unknown) => void;
    native.api[method].mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve; }));
    const first = simulator.initialize();
    await vi.waitFor(() => expect(native.api[method]).toHaveBeenCalledTimes(1));
    simulator.dispose();
    await simulator.initialize();
    const currentPreview = simulator.preview.value;
    const currentAssets = simulator.assets.value;
    resolveOld(method === 'previewDevices' ? { devices: [], total_devices: 999 } : { state: 'failed' });
    await first;
    expect(simulator.preview.value).toBe(currentPreview);
    expect(simulator.assets.value).toBe(currentAssets);
    expect(native.api.getSettings).toHaveBeenCalledTimes(2);
  });

  it('shares an in-flight initialization and processes each event once', async () => {
    const first = simulator.initialize();
    const second = simulator.initialize();
    expect(pending).toHaveLength(8);
    const stops = pending.map(() => vi.fn());
    pending.forEach((registration, index) => registration.resolve(stops[index]));
    await Promise.all([first, second]);
    expect(native.api.getSettings).toHaveBeenCalledTimes(1);
    const logs = native.listen.mock.calls.filter(([name]) => name === 'device-simulator-log');
    expect(logs).toHaveLength(1);
    logs[0][1]({ payload: { message: 'one event' } });
    expect(simulator.logs.value).toHaveLength(1);
    simulator.dispose();
    stops.forEach((stop) => expect(stop).toHaveBeenCalledTimes(1));
  });

  it('cleans every successful registration after a partial failure, then allows retry', async () => {
    const initialization = simulator.initialize();
    const rejected = expect(initialization).rejects.toThrow('registration failed');
    pending[0].reject(new Error('registration failed'));
    const stops = pending.slice(1).map(() => vi.fn());
    pending.slice(1).forEach((registration, index) => registration.resolve(stops[index]));
    await rejected;
    stops.forEach((stop) => expect(stop).toHaveBeenCalledTimes(1));
    native.listen.mockImplementation(async () => vi.fn());
    await simulator.initialize();
    expect(native.listen).toHaveBeenCalledTimes(16);
    expect(native.api.getSettings).toHaveBeenCalledTimes(1);
  });

  it('cleans late registrations after disposal without removing a replacement initialization', async () => {
    const first = simulator.initialize();
    const oldRegistrations = [...pending];
    simulator.dispose();
    pending = [];
    const second = simulator.initialize();
    const currentStops = pending.map(() => vi.fn());
    pending.forEach((registration, index) => registration.resolve(currentStops[index]));
    await second;
    const oldStops = oldRegistrations.map(() => vi.fn());
    oldRegistrations.forEach((registration, index) => registration.resolve(oldStops[index]));
    await first;
    oldStops.forEach((stop) => expect(stop).toHaveBeenCalledTimes(1));
    currentStops.forEach((stop) => expect(stop).not.toHaveBeenCalled());
    expect(native.api.getSettings).toHaveBeenCalledTimes(1);
    simulator.dispose();
    currentStops.forEach((stop) => expect(stop).toHaveBeenCalledTimes(1));
  });
});
