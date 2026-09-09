import { reactive } from 'vue';

import type { UmsInitPasswordKind } from './tauri';

/**
 * Form state for the UMS initial password tool.
 *
 * Lives at module scope so switching tabs (which unmounts the page) keeps
 * everything the user typed. Follows the same pattern as `manualCopyFormState`.
 */
export interface UmsInitialPasswordFormState {
  selectedIps: string[];
  manualIpTags: string[];
  manualIpInput: string;
  enabledFlows: Record<UmsInitPasswordKind, boolean>;
  umsUsername: string;
  newPasswords: Record<UmsInitPasswordKind, string>;
  oldPasswords: Record<UmsInitPasswordKind, string>;
}

/** Factory defaults, one per flow — each appliance ships with a different one. */
export const DEFAULT_OLD_PASSWORDS: Record<UmsInitPasswordKind, string> = {
  framework: '123456',
  ums: 'admin_123',
  cdm: 'admin',
};

export const DEFAULT_UMS_USERNAME = 'loadmin';

export const DEFAULT_NEW_PASSWORDS: Record<UmsInitPasswordKind, string> = {
  framework: 'admin_123',
  ums: 'admin_1234',
  cdm: 'admin_123',
};

const STORAGE_KEY = 'umsInitialPassword_form_state';

/** Only non-secret fields are persisted; see `persist()`. */
interface PersistedShape {
  selectedIps: string[];
  manualIpTags: string[];
  manualIpInput: string;
  enabledFlows: Record<UmsInitPasswordKind, boolean>;
  umsUsername: string;
}

function defaultState(): UmsInitialPasswordFormState {
  return {
    selectedIps: [],
    manualIpTags: [],
    manualIpInput: '',
    enabledFlows: { framework: true, ums: true, cdm: true },
    umsUsername: DEFAULT_UMS_USERNAME,
    newPasswords: { ...DEFAULT_NEW_PASSWORDS },
    oldPasswords: { ...DEFAULT_OLD_PASSWORDS },
  };
}

function toStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === 'string');
}

function toFlowFlags(value: unknown): Record<UmsInitPasswordKind, boolean> {
  const source = (value ?? {}) as Partial<Record<UmsInitPasswordKind, unknown>>;
  const read = (kind: UmsInitPasswordKind) =>
    typeof source[kind] === 'boolean' ? (source[kind] as boolean) : true;
  return {
    framework: read('framework'),
    ums: read('ums'),
    cdm: read('cdm'),
  };
}

function loadFromStorage(): UmsInitialPasswordFormState {
  const state = defaultState();
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (!stored) return state;
    const parsed = JSON.parse(stored) as Partial<PersistedShape>;
    state.selectedIps = toStringArray(parsed.selectedIps);
    state.manualIpTags = toStringArray(parsed.manualIpTags);
    state.manualIpInput = typeof parsed.manualIpInput === 'string' ? parsed.manualIpInput : '';
    state.enabledFlows = toFlowFlags(parsed.enabledFlows);
    state.umsUsername = typeof parsed.umsUsername === 'string' && parsed.umsUsername.trim()
      ? parsed.umsUsername
      : DEFAULT_UMS_USERNAME;
  } catch {
    // Ignore malformed state from older builds and start clean.
  }
  return state;
}

export const umsInitialPasswordFormState = reactive<UmsInitialPasswordFormState>(loadFromStorage());

/**
 * Persist the target selection across app restarts.
 *
 * Passwords are deliberately left out — writing them to localStorage would put
 * plaintext credentials on disk for no real benefit, since the factory defaults
 * are the common case and are restored automatically. They still survive tab
 * switches because the reactive state itself is module scoped.
 */
export function persistUmsInitialPasswordForm(): void {
  const payload: PersistedShape = {
    selectedIps: [...umsInitialPasswordFormState.selectedIps],
    manualIpTags: [...umsInitialPasswordFormState.manualIpTags],
    manualIpInput: umsInitialPasswordFormState.manualIpInput,
    enabledFlows: { ...umsInitialPasswordFormState.enabledFlows },
    umsUsername: umsInitialPasswordFormState.umsUsername,
  };
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(payload));
  } catch {
    // Persistence is best-effort only.
  }
}

export function resetUmsInitialPasswordForm(): void {
  Object.assign(umsInitialPasswordFormState, defaultState());
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    // Ignore storage failures.
  }
}
