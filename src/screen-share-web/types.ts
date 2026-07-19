export type AnnotationKind = 'laser' | 'arrow' | 'rect';

export type ViewMode = 'live' | 'frozen';

export interface NormalizedPoint {
  x: number;
  y: number;
}

export interface AnnotationShape {
  id: string;
  ownerClientId: string;
  kind: AnnotationKind;
  points: NormalizedPoint[];
  color: string;
  width: number;
  expiresAtMs: number | null;
}

export interface AnnotationDocument {
  sessionId: number;
  sourceEpoch: number;
  revision: number;
  mode: ViewMode;
  frozenFrameId: number | null;
  shapes: AnnotationShape[];
}

export interface SessionFeatures {
  annotations_enabled?: boolean;
  shared_freeze_enabled?: boolean;
  control_requests?: boolean;
  keyboard_control?: boolean;
  [key: string]: unknown;
}

export interface SessionEnvelope<T = unknown> {
  v: 1;
  type: string;
  session_id: number;
  source_epoch: number;
  client_seq?: number;
  revision?: number;
  payload?: T;
}

export interface SessionHelloPayload {
  client_id: string;
  session_id?: number;
  source_epoch?: number;
  frame_id?: number | null;
  width?: number;
  height?: number;
  features?: SessionFeatures;
}

export interface AnnotationAddPayload {
  kind: AnnotationKind;
  points: NormalizedPoint[];
  color: string;
  width: number;
  expires_at_ms?: number | null;
}

export interface AnnotationAppliedPayload {
  operation?: 'add' | 'remove' | 'clear_own' | 'clear_all' | string;
  shape?: AnnotationShape;
  removed_ids?: string[];
  owner_client_id?: string;
  document?: AnnotationDocument;
  shapes?: AnnotationShape[];
}

export interface ViewStatePayload {
  document?: AnnotationDocument;
  snapshot_url?: string | null;
  mode?: ViewMode;
  frame_id?: number | null;
  frozen_frame_id?: number | null;
}

export interface SourceChangedPayload {
  source_epoch?: number;
  width?: number;
  height?: number;
}

export interface SessionErrorPayload {
  code?: string;
  message?: string;
  retryable?: boolean;
}

export type SessionServerMessage = SessionEnvelope<
  SessionHelloPayload
  | AnnotationDocument
  | AnnotationAppliedPayload
  | ViewStatePayload
  | SourceChangedPayload
  | SessionErrorPayload
  | Record<string, unknown>
>;

export interface ScreenShareHttpStatus {
  active?: boolean;
  is_active?: boolean;
  viewers?: number;
  viewer_count?: number;
  session_id?: number;
  source_epoch?: number;
  annotation_count?: number;
  view_mode?: ViewMode;
  frozen_frame_id?: number | null;
  interaction_connected_count?: number;
  latest_frame_id?: number | null;
  frame_width?: number | null;
  frame_height?: number | null;
  capture_paused?: boolean;
  capture_issue?: 'retrying' | 'privacy_mode_or_display_off' | string | null;
}

export interface SessionConnectionState {
  status: 'idle' | 'connecting' | 'connected' | 'reconnecting' | 'closed';
  attempts: number;
  lastError: string | null;
}

export function isAnnotationKind(value: unknown): value is AnnotationKind {
  return value === 'laser' || value === 'arrow' || value === 'rect';
}

export function isViewMode(value: unknown): value is ViewMode {
  return value === 'live' || value === 'frozen';
}

export function finiteUnit(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= 1;
}
