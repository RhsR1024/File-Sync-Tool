import { afterEach, describe, expect, it } from 'vitest';

import {
  SCREEN_SHARE_DIAGNOSTICS_GLOBAL,
  installScreenShareDiagnostics,
} from './diagnostics';

afterEach(() => {
  delete window.__SCREEN_SHARE_DIAGNOSTICS__;
});

describe('screen-share diagnostics global', () => {
  it('installs a fixed-name read-only snapshot API and removes only itself', () => {
    const source = {
      capturedAtUnixMs: 123,
      transport: 'mse_h264',
      server: { encoded: 4, nested: { samples: [1, 2] } },
      client: { queued: 0 },
    };
    const uninstall = installScreenShareDiagnostics(() => ({
      ...source,
    }));
    const api = window[SCREEN_SHARE_DIAGNOSTICS_GLOBAL];
    expect(Object.isFrozen(api)).toBe(true);
    expect(api?.snapshot()).toEqual({
      capturedAtUnixMs: 123,
      transport: 'mse_h264',
      server: { encoded: 4, nested: { samples: [1, 2] } },
      client: { queued: 0 },
    });
    const snapshot = api?.snapshot();
    expect(Object.isFrozen(snapshot)).toBe(true);
    expect(Object.isFrozen(snapshot?.server)).toBe(true);
    expect(Object.isFrozen((snapshot?.server.nested as { samples: number[] }).samples)).toBe(true);
    expect(snapshot?.server).not.toBe(source.server);
    expect(() => {
      (snapshot?.server as { encoded: number }).encoded = 9;
    }).toThrow();
    expect(source.server.encoded).toBe(4);
    uninstall();
    expect(window.__SCREEN_SHARE_DIAGNOSTICS__).toBeUndefined();
  });
});
