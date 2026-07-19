import {
  finiteUnit,
  isAnnotationKind,
  isViewMode,
  type AnnotationAppliedPayload,
  type AnnotationDocument,
  type AnnotationShape,
  type NormalizedPoint,
} from '../types';

const MAX_SHAPES = 200;
const MAX_POINTS = 256;

export function emptyDocument(sessionId = 0, sourceEpoch = 0): AnnotationDocument {
  return {
    sessionId,
    sourceEpoch,
    revision: 0,
    mode: 'live',
    frozenFrameId: null,
    shapes: [],
  };
}

function normalizePoint(value: unknown): NormalizedPoint | null {
  if (!value || typeof value !== 'object') {
    return null;
  }
  const point = value as Record<string, unknown>;
  if (!finiteUnit(point.x) || !finiteUnit(point.y)) {
    return null;
  }
  return { x: point.x, y: point.y };
}

export function normalizeShape(value: unknown): AnnotationShape | null {
  if (!value || typeof value !== 'object') {
    return null;
  }
  const source = value as Record<string, unknown>;
  const ownerClientId = typeof source.ownerClientId === 'string'
    ? source.ownerClientId
    : (typeof source.owner_client_id === 'string' ? source.owner_client_id : '');
  if (typeof source.id !== 'string' || !ownerClientId) {
    return null;
  }
  const kind = source.kind;
  if (!isAnnotationKind(kind)) {
    return null;
  }
  const points = Array.isArray(source.points)
    ? source.points.slice(0, MAX_POINTS).map(normalizePoint).filter((point): point is NormalizedPoint => point !== null)
    : [];
  if ((kind === 'laser' && points.length < 1) || (kind !== 'laser' && points.length < 2)) {
    return null;
  }
  const width = typeof source.width === 'number' && Number.isFinite(source.width)
    ? Math.min(24, Math.max(1, source.width))
    : 3;
  const rawExpiresAt = source.expiresAtMs ?? source.expires_at_ms;
  const expiresAtMs = rawExpiresAt === null || rawExpiresAt === undefined
    ? null
    : (typeof rawExpiresAt === 'number' && Number.isFinite(rawExpiresAt) ? rawExpiresAt : null);
  return {
    id: source.id,
    ownerClientId,
    kind,
    points,
    color: typeof source.color === 'string' && source.color.trim() ? source.color : '#f59e0b',
    width,
    expiresAtMs,
  };
}

/** Accepts both the documented camelCase document and serde snake_case variants. */
export function normalizeDocument(value: unknown, fallback?: AnnotationDocument): AnnotationDocument {
  const base = fallback ?? emptyDocument();
  if (!value || typeof value !== 'object') {
    return { ...base, shapes: [...base.shapes] };
  }
  const source = value as Record<string, unknown>;
  const sessionId = numberOr(source.sessionId, source.session_id, base.sessionId);
  const sourceEpoch = numberOr(source.sourceEpoch, source.source_epoch, base.sourceEpoch);
  const revision = Math.max(0, numberOr(source.revision, undefined, base.revision));
  const mode = isViewMode(source.mode) ? source.mode : base.mode;
  const frozenValue = source.frozenFrameId ?? source.frozen_frame_id;
  const frozenFrameId = typeof frozenValue === 'number' && Number.isFinite(frozenValue) ? frozenValue : null;
  const rawShapes = Array.isArray(source.shapes) ? source.shapes : [];
  const shapes = rawShapes
    .map(normalizeShape)
    .filter((shape): shape is AnnotationShape => shape !== null)
    .slice(-MAX_SHAPES);
  return { sessionId, sourceEpoch, revision, mode, frozenFrameId, shapes };
}

function numberOr(first: unknown, second: unknown, fallback: number): number {
  if (typeof first === 'number' && Number.isFinite(first)) return first;
  if (typeof second === 'number' && Number.isFinite(second)) return second;
  return fallback;
}

export function visibleShapes(document: AnnotationDocument, now = Date.now()): AnnotationShape[] {
  return document.shapes.filter((shape) => shape.expiresAtMs === null || shape.expiresAtMs > now);
}

export function ownShapes(document: AnnotationDocument, clientId: string): AnnotationShape[] {
  return document.shapes.filter((shape) => shape.ownerClientId === clientId);
}

export function applySnapshot(
  current: AnnotationDocument,
  incoming: unknown,
): AnnotationDocument {
  return normalizeDocument(incoming, current);
}

export interface ApplyResult {
  document: AnnotationDocument;
  needsSnapshot: boolean;
}

export function applyAnnotationApplied(
  current: AnnotationDocument,
  payload: AnnotationAppliedPayload | unknown,
  revision: number | undefined,
): ApplyResult {
  const source = (payload && typeof payload === 'object' ? payload : {}) as Record<string, unknown>;
  if (source.document) {
    return { document: normalizeDocument(source.document, current), needsSnapshot: false };
  }
  if (revision !== undefined && revision !== current.revision + 1) {
    return { document: current, needsSnapshot: true };
  }
  const next = { ...current, shapes: [...current.shapes], revision: revision ?? current.revision + 1 };
  const operation = typeof source.operation === 'string' ? source.operation : 'add';
  const shape = normalizeShape(source.shape);
  if (operation === 'add' && shape) {
    next.shapes = [...next.shapes.filter((item) => item.id !== shape.id), shape].slice(-MAX_SHAPES);
  } else if (operation === 'remove') {
    const ids = new Set(Array.isArray(source.removed_ids) ? source.removed_ids.filter((id): id is string => typeof id === 'string') : []);
    if (shape) ids.add(shape.id);
    next.shapes = next.shapes.filter((item) => !ids.has(item.id));
  } else if (operation === 'clear_own') {
    const owner = typeof source.owner_client_id === 'string' ? source.owner_client_id : '';
    if (owner) next.shapes = next.shapes.filter((item) => item.ownerClientId !== owner);
  } else if (operation === 'clear_all') {
    next.shapes = [];
  } else if (Array.isArray(source.shapes)) {
    next.shapes = source.shapes
      .map(normalizeShape)
      .filter((item): item is AnnotationShape => item !== null)
      .slice(-MAX_SHAPES);
  }
  return { document: next, needsSnapshot: false };
}

export function applyViewState(
  current: AnnotationDocument,
  mode: unknown,
  frameId: unknown,
): AnnotationDocument {
  const nextMode = isViewMode(mode) ? mode : current.mode;
  const nextFrame = typeof frameId === 'number' && Number.isFinite(frameId) ? frameId : null;
  return {
    ...current,
    mode: nextMode,
    frozenFrameId: nextMode === 'frozen' ? nextFrame : null,
    shapes: nextMode === 'live'
      ? current.shapes.filter((shape) => shape.kind === 'laser')
      : [...current.shapes],
  };
}

export function resetForSource(
  current: AnnotationDocument,
  sourceEpoch: number,
): AnnotationDocument {
  return {
    ...current,
    sourceEpoch,
    revision: 0,
    mode: 'live',
    frozenFrameId: null,
    shapes: [],
  };
}
