import { describe, expect, it } from 'vitest';

import {
  canHandleRemoteInput,
  decideRemoteKeyboardAction,
  mergeControlSnapshot,
  mergeHttpControlSnapshot,
  normalizeRemoteWheelDelta,
  remoteMouseButton,
  toRemoteKeyboardCode,
} from './remote-control';

describe('remote control browser input', () => {
  it('keeps WebSocket controller identity when HTTP status omits it', () => {
    const granted = {
      state: 'granted' as const,
      controller_client_id: 'viewer-a',
      controller_ip: '192.168.1.20',
    };

    expect(mergeControlSnapshot(granted, {
      state: 'granted',
      controller_ip: '192.168.1.20',
    })).toMatchObject(granted);

    expect(mergeControlSnapshot(granted, { state: 'revoked' })).toMatchObject({
      state: 'revoked',
      controller_client_id: undefined,
      controller_ip: undefined,
    });
  });

  it('ignores delayed HTTP control state while the interaction socket is connected', () => {
    const granted = {
      state: 'granted' as const,
      controller_client_id: 'viewer-a',
      controller_ip: '192.168.1.20',
    };

    expect(mergeHttpControlSnapshot(granted, 'requested', null, true)).toBeNull();
    expect(mergeHttpControlSnapshot(granted, 'revoked', null, false)).toMatchObject({
      state: 'revoked',
      controller_client_id: undefined,
    });
  });

  it('keeps view, annotation and control modes mutually exclusive', () => {
    const base = {
      isController: true,
      connected: true,
      localPaused: false,
      sharedFrozen: false,
    };

    expect(canHandleRemoteInput({ ...base, tool: 'control' })).toBe(true);
    expect(canHandleRemoteInput({ ...base, tool: 'arrow' })).toBe(false);
    expect(canHandleRemoteInput({ ...base, tool: 'view' })).toBe(false);
    expect(canHandleRemoteInput({ ...base, tool: 'control', isController: false })).toBe(false);
    expect(canHandleRemoteInput({ ...base, tool: 'control', sharedFrozen: true })).toBe(false);
    expect(canHandleRemoteInput({ ...base, tool: 'control', localPaused: true })).toBe(false);
  });

  it('accepts only left/right buttons and clamps wheel input', () => {
    expect(remoteMouseButton(0)).toBe('left');
    expect(remoteMouseButton(2)).toBe('right');
    expect(remoteMouseButton(1)).toBeNull();
    expect(normalizeRemoteWheelDelta(120)).toBe(-120);
    expect(normalizeRemoteWheelDelta(-5000)).toBe(1200);
    expect(normalizeRemoteWheelDelta(Number.NaN)).toBeNull();
  });

  it('forwards every identified browser keyboard code within the protocol bound', () => {
    expect(toRemoteKeyboardCode('KeyA')).toBe('KeyA');
    expect(toRemoteKeyboardCode('Digit7')).toBe('Digit7');
    expect(toRemoteKeyboardCode('Semicolon')).toBe('Semicolon');
    expect(toRemoteKeyboardCode('F24')).toBe('F24');
    expect(toRemoteKeyboardCode('MetaLeft')).toBe('MetaLeft');
    expect(toRemoteKeyboardCode('AudioVolumeUp')).toBe('AudioVolumeUp');
    expect(toRemoteKeyboardCode('Unidentified')).toBeNull();
    expect(toRemoteKeyboardCode('')).toBeNull();
    expect(toRemoteKeyboardCode('K'.repeat(33))).toBeNull();
  });

  it('forwards only supported key edges and ignores untracked releases', () => {
    expect(decideRemoteKeyboardAction(
      { code: 'KeyA', pressed: true },
      new Set(),
    )).toEqual({ type: 'key', code: 'KeyA', pressed: true });
    expect(decideRemoteKeyboardAction(
      { code: 'KeyA', pressed: false },
      new Set(['KeyA']),
    )).toEqual({ type: 'key', code: 'KeyA', pressed: false });
    expect(decideRemoteKeyboardAction(
      { code: 'KeyA', pressed: false },
      new Set(),
    )).toEqual({ type: 'ignore' });
    expect(decideRemoteKeyboardAction(
      { code: 'F1', pressed: true },
      new Set(),
    )).toEqual({ type: 'key', code: 'F1', pressed: true });
  });

  it('forwards system combinations and ignores only unidentified or composing input', () => {
    expect(decideRemoteKeyboardAction(
      { code: 'Tab', pressed: true },
      new Set(['AltLeft']),
    )).toEqual({ type: 'key', code: 'Tab', pressed: true });
    expect(decideRemoteKeyboardAction(
      { code: 'Escape', pressed: true },
      new Set(['ControlLeft']),
    )).toEqual({ type: 'key', code: 'Escape', pressed: true });
    expect(decideRemoteKeyboardAction(
      { code: 'MetaLeft', pressed: true },
      new Set(),
    )).toEqual({ type: 'key', code: 'MetaLeft', pressed: true });
    expect(decideRemoteKeyboardAction(
      { code: 'Unidentified', pressed: true },
      new Set(['ShiftLeft']),
    )).toEqual({ type: 'ignore' });
    expect(decideRemoteKeyboardAction(
      { code: 'KeyA', pressed: true, composing: true },
      new Set(),
    )).toEqual({ type: 'ignore' });
  });
});
