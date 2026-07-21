export type AnnotationKind = 'laser' | 'arrow' | 'rect';

export type ViewMode = 'live' | 'frozen';
export type ControlState = 'disabled' | 'available' | 'requested' | 'granted' | 'revoked';

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
  control_requests_enabled?: boolean;
  keyboard_control_enabled?: boolean;
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

export interface AnnotationRemovePayload {
  shape_id: string;
}

export interface AnnotationUpdatePayload {
  shape_id: string;
  points: NormalizedPoint[];
  color: string;
  width: number;
}

export interface AnnotationAppliedPayload {
  operation?: 'add' | 'update' | 'remove' | 'undo' | 'clear_own' | 'clear_all' | string;
  shape?: AnnotationShape;
  removed_ids?: string[];
  owner_client_id?: string;
  document?: AnnotationDocument;
  shapes?: AnnotationShape[];
}

export interface ViewStatePayload {
  document?: AnnotationDocument;
  control?: ControlStateSnapshot;
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

export interface ControlStateSnapshot {
  state: ControlState;
  request_id?: string;
  requester_client_id?: string;
  controller_client_id?: string;
  controller_ip?: string;
}

export interface ControlRequestedPayload {
  request_id?: string;
  client_id?: string;
  ip?: string;
  user_agent?: string;
}

export interface ControlStatePayload {
  control?: ControlStateSnapshot;
  reason?: string;
}

export type SessionServerMessage = SessionEnvelope<
  SessionHelloPayload
  | AnnotationDocument
  | AnnotationAppliedPayload
  | ViewStatePayload
  | SourceChangedPayload
  | SessionErrorPayload
  | ControlRequestedPayload
  | ControlStatePayload
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
  transport?: 'mjpeg' | 'mse_h264' | 'webrtc';
  h264_media?: {
    ready?: boolean;
    codec?: string | null;
    width?: number | null;
    height?: number | null;
    target_bitrate_bps?: number | null;
    encoded_frame_count?: number;
    encoded_bytes?: number;
    keyframe_count?: number;
    cached_segment_count?: number;
    cached_bytes?: number;
    dropped_input_frames?: number;
    error?: string | null;
  };
  control_state?: ControlState;
  controller_ip?: string | null;
}

export interface H264MediaHello {
  v: 1;
  type: 'media.hello';
  transport: 'mse_h264';
  generation: number;
  codec: string;
  mime_type: string;
  width: number;
  height: number;
  fps: number;
  bitrate_bps: number;
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
