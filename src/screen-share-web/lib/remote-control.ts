import type { AnnotationKind, ControlStateSnapshot } from '../types';

export type ScreenShareTool = AnnotationKind | 'view' | 'control';

/**
 * Merge a partial status update without discarding identity from the current
 * WebSocket snapshot. A state transition invalidates the old ownership data.
 */
export function mergeControlSnapshot(
  current: ControlStateSnapshot,
  incoming: Pick<ControlStateSnapshot, 'state'> & Partial<Omit<ControlStateSnapshot, 'state'>>,
): ControlStateSnapshot {
  const sameState = current.state === incoming.state;
  return {
    state: incoming.state,
    request_id: incoming.request_id ?? (sameState ? current.request_id : undefined),
    requester_client_id: incoming.requester_client_id ?? (sameState ? current.requester_client_id : undefined),
    controller_client_id: incoming.controller_client_id ?? (sameState ? current.controller_client_id : undefined),
    controller_ip: incoming.controller_ip ?? (sameState ? current.controller_ip : undefined),
  };
}

export function mergeHttpControlSnapshot(
  current: ControlStateSnapshot,
  state: ControlStateSnapshot['state'],
  controllerIp: string | null | undefined,
  interactionConnected: boolean,
): ControlStateSnapshot | null {
  if (interactionConnected) return null;
  return mergeControlSnapshot(current, {
    state,
    controller_ip: controllerIp ?? undefined,
  });
}

export interface RemoteControlModeState {
  tool: ScreenShareTool;
  isController: boolean;
  connected: boolean;
  localPaused: boolean;
  sharedFrozen: boolean;
}

export interface RemoteKeyboardInput {
  code: string;
  pressed: boolean;
  metaKey?: boolean;
  composing?: boolean;
}

export type RemoteKeyboardAction =
  | { type: 'key'; code: string; pressed: boolean }
  | { type: 'release_all' }
  | { type: 'ignore' };

const FIXED_KEY_CODES = new Set([
  'ArrowLeft',
  'ArrowRight',
  'ArrowUp',
  'ArrowDown',
  'Enter',
  'Escape',
  'Backspace',
  'Tab',
  'Space',
  'ControlLeft',
  'ControlRight',
  'ShiftLeft',
  'ShiftRight',
  'AltLeft',
  'AltRight',
]);

export function toRemoteKeyboardCode(code: string): string | null {
  if (/^Key[A-Z]$/.test(code) || /^Digit[0-9]$/.test(code) || FIXED_KEY_CODES.has(code)) {
    return code;
  }
  return null;
}

export function isRemoteKeyboardModifier(code: string): boolean {
  return code.startsWith('Control') || code.startsWith('Shift') || code.startsWith('Alt');
}

export function isRestrictedRemoteShortcut(code: string, pressed: ReadonlySet<string>): boolean {
  const hasAlt = [...pressed].some((item) => item.startsWith('Alt'));
  const hasControl = [...pressed].some((item) => item.startsWith('Control'));
  if (code === 'Tab' && hasAlt) return true;
  if (code === 'Escape' && (hasAlt || hasControl)) return true;
  return false;
}

export function decideRemoteKeyboardAction(
  input: RemoteKeyboardInput,
  forwarded: ReadonlySet<string>,
): RemoteKeyboardAction {
  if (input.composing) return { type: 'ignore' };

  const code = toRemoteKeyboardCode(input.code);
  if (!input.pressed) {
    return code && forwarded.has(code)
      ? { type: 'key', code, pressed: false }
      : { type: 'ignore' };
  }

  if (input.metaKey) return { type: 'release_all' };
  if (!code) {
    return forwarded.size > 0 ? { type: 'release_all' } : { type: 'ignore' };
  }
  if (isRestrictedRemoteShortcut(code, forwarded)) return { type: 'release_all' };
  return { type: 'key', code, pressed: true };
}

export function canHandleRemoteInput(state: RemoteControlModeState): boolean {
  return state.tool === 'control'
    && state.isController
    && state.connected
    && !state.localPaused
    && !state.sharedFrozen;
}

export function remoteMouseButton(button: number): 'left' | 'right' | null {
  if (button === 0) return 'left';
  if (button === 2) return 'right';
  return null;
}

export function normalizeRemoteWheelDelta(deltaY: number): number | null {
  if (!Number.isFinite(deltaY) || deltaY === 0) return null;
  const delta = Math.max(-1200, Math.min(1200, Math.round(-deltaY)));
  return delta || null;
}
