//! Server-authoritative collaboration state for the screen-share session.
//!
//! The media path (currently MJPEG) deliberately does not depend on this
//! module.  This module only owns the small, bounded interaction document and
//! the protocol messages sent over the collaboration WebSocket.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

pub const PROTOCOL_VERSION: u8 = 1;
pub const MAX_WS_MESSAGE_BYTES: usize = 64 * 1024;
pub const MAX_CLIENTS: usize = 64;
pub const MAX_SHAPES: usize = 200;
pub const MAX_POINTS: usize = 256;
pub const MAX_CLIENT_ID_BYTES: usize = 96;
pub const LASER_TTL_MS: u64 = 2_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ViewMode {
    Live,
    Frozen,
}

impl Default for ViewMode {
    fn default() -> Self {
        Self::Live
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationKind {
    Laser,
    Arrow,
    Rect,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NormalizedPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnnotationShape {
    pub id: String,
    pub owner_client_id: String,
    pub kind: AnnotationKind,
    pub points: Vec<NormalizedPoint>,
    pub color: String,
    pub width: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnnotationDocument {
    pub session_id: u64,
    pub source_epoch: u64,
    pub revision: u64,
    pub mode: ViewMode,
    pub frozen_frame_id: Option<u64>,
    pub shapes: Vec<AnnotationShape>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnnotationAddPayload {
    pub kind: AnnotationKind,
    pub points: Vec<NormalizedPoint>,
    pub color: String,
    pub width: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClientEnvelope {
    pub v: u8,
    #[serde(rename = "type")]
    pub message_type: String,
    pub session_id: u64,
    pub source_epoch: u64,
    #[serde(default)]
    pub client_seq: Option<u64>,
    #[serde(default)]
    pub revision: Option<u64>,
    #[serde(default)]
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ServerEnvelope {
    pub v: u8,
    #[serde(rename = "type")]
    pub message_type: String,
    pub session_id: u64,
    pub source_epoch: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

impl ServerEnvelope {
    fn new(
        message_type: impl Into<String>,
        session_id: u64,
        source_epoch: u64,
        revision: Option<u64>,
        payload: Option<serde_json::Value>,
    ) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            message_type: message_type.into(),
            session_id,
            source_epoch,
            client_seq: None,
            revision,
            payload,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SessionHelloPayload {
    pub client_id: String,
    pub features: InteractionFeatures,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InteractionFeatures {
    pub annotations_enabled: bool,
    pub shared_freeze_enabled: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AnnotationAppliedPayload {
    pub operation: String,
    pub document: AnnotationDocument,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ViewStatePayload {
    pub document: AnnotationDocument,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SessionErrorPayload {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ProtocolError {
    pub code: &'static str,
    pub message: String,
}

impl ProtocolError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn to_message(&self, state: &InteractionState) -> ServerEnvelope {
        let (session_id, source_epoch, revision) = state.identity();
        ServerEnvelope::new(
            "session.error",
            session_id,
            source_epoch,
            Some(revision),
            Some(
                serde_json::to_value(SessionErrorPayload {
                    code: self.code.to_string(),
                    message: self.message.clone(),
                })
                .expect("session error payload is serializable"),
            ),
        )
    }
}

#[derive(Debug, Clone)]
pub struct StoredFrame {
    pub frame_id: u64,
    pub source_epoch: u64,
    pub captured_at_ms: u64,
    pub width: u32,
    pub height: u32,
    pub bytes: Arc<Bytes>,
}

#[derive(Debug)]
struct InteractionInner {
    document: AnnotationDocument,
    clients: HashSet<String>,
    last_client_seq: HashMap<String, u64>,
    latest_frame: Option<StoredFrame>,
    frozen_frame: Option<StoredFrame>,
    next_frame_id: u64,
}

/// Bounded, in-memory state for one screen-share session.
pub struct InteractionState {
    inner: Mutex<InteractionInner>,
    events: broadcast::Sender<ServerEnvelope>,
}

impl std::fmt::Debug for InteractionState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InteractionState")
            .field("identity", &self.identity())
            .finish_non_exhaustive()
    }
}

impl InteractionState {
    pub fn new(session_id: u64) -> Arc<Self> {
        let (events, _) = broadcast::channel(128);
        Arc::new(Self {
            inner: Mutex::new(InteractionInner {
                document: AnnotationDocument {
                    session_id,
                    source_epoch: 1,
                    revision: 0,
                    mode: ViewMode::Live,
                    frozen_frame_id: None,
                    shapes: Vec::new(),
                },
                clients: HashSet::new(),
                last_client_seq: HashMap::new(),
                latest_frame: None,
                frozen_frame: None,
                next_frame_id: 0,
            }),
            events,
        })
    }

    pub fn identity(&self) -> (u64, u64, u64) {
        let inner = self.inner.lock().expect("interaction state lock poisoned");
        (
            inner.document.session_id,
            inner.document.source_epoch,
            inner.document.revision,
        )
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServerEnvelope> {
        self.events.subscribe()
    }

    pub fn register_client(&self, client_id: &str) -> Result<(), ProtocolError> {
        validate_client_id(client_id)?;
        let mut inner = self.inner.lock().expect("interaction state lock poisoned");
        if inner.clients.contains(client_id) {
            return Err(ProtocolError::new(
                "client_exists",
                "client is already registered",
            ));
        }
        if inner.clients.len() >= MAX_CLIENTS {
            return Err(ProtocolError::new(
                "client_limit_reached",
                format!("at most {MAX_CLIENTS} interaction clients are allowed"),
            ));
        }
        inner.clients.insert(client_id.to_string());
        Ok(())
    }

    pub fn unregister_client(&self, client_id: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.clients.remove(client_id);
            inner.last_client_seq.remove(client_id);
        }
    }

    pub fn client_count(&self) -> usize {
        self.inner
            .lock()
            .map(|inner| inner.clients.len())
            .unwrap_or_default()
    }

    pub fn snapshot(&self) -> AnnotationDocument {
        let mut inner = self.inner.lock().expect("interaction state lock poisoned");
        expire_lasers_locked(&mut inner);
        inner.document.clone()
    }

    pub fn hello(&self, client_id: &str) -> Result<ServerEnvelope, ProtocolError> {
        let inner = self.inner.lock().expect("interaction state lock poisoned");
        if !inner.clients.contains(client_id) {
            return Err(ProtocolError::new(
                "client_not_registered",
                "client must be registered before hello",
            ));
        }
        Ok(ServerEnvelope::new(
            "session.hello",
            inner.document.session_id,
            inner.document.source_epoch,
            Some(inner.document.revision),
            Some(
                serde_json::to_value(SessionHelloPayload {
                    client_id: client_id.to_string(),
                    features: InteractionFeatures {
                        annotations_enabled: true,
                        shared_freeze_enabled: true,
                    },
                })
                .expect("hello payload is serializable"),
            ),
        ))
    }

    pub fn snapshot_message(&self) -> ServerEnvelope {
        let document = self.snapshot();
        ServerEnvelope::new(
            "session.snapshot",
            document.session_id,
            document.source_epoch,
            Some(document.revision),
            Some(serde_json::json!({
                "document": document,
            })),
        )
    }

    pub fn process(
        &self,
        client_id: &str,
        envelope: ClientEnvelope,
    ) -> Result<Option<ServerEnvelope>, ProtocolError> {
        if envelope.v != PROTOCOL_VERSION {
            return Err(ProtocolError::new(
                "unsupported_protocol_version",
                format!("expected protocol version {PROTOCOL_VERSION}"),
            ));
        }

        let mut inner = self.inner.lock().expect("interaction state lock poisoned");
        validate_client_locked(&inner, client_id)?;
        if envelope.session_id != inner.document.session_id {
            return Err(ProtocolError::new(
                "session_mismatch",
                "session_id does not match",
            ));
        }
        if envelope.source_epoch != inner.document.source_epoch {
            return Err(ProtocolError::new(
                "source_epoch_mismatch",
                "source_epoch does not match the current capture source",
            ));
        }
        if envelope
            .revision
            .is_some_and(|revision| revision > inner.document.revision)
        {
            return Err(ProtocolError::new(
                "revision_ahead",
                "client revision is newer than the server document",
            ));
        }
        if let Some(seq) = envelope.client_seq {
            if inner
                .last_client_seq
                .get(client_id)
                .is_some_and(|last| seq <= *last)
            {
                return Err(ProtocolError::new(
                    "stale_client_seq",
                    "client_seq must increase for each message",
                ));
            }
            inner.last_client_seq.insert(client_id.to_string(), seq);
        }

        expire_lasers_locked(&mut inner);
        let message_type = envelope.message_type.as_str();
        let result = match message_type {
            "session.heartbeat" => None,
            "annotation.add" => {
                let payload = parse_payload::<AnnotationAddPayload>(&envelope)?;
                validate_add_payload(&payload)?;
                if inner.document.shapes.len() >= MAX_SHAPES {
                    return Err(ProtocolError::new(
                        "shape_limit_reached",
                        format!("at most {MAX_SHAPES} annotations are allowed"),
                    ));
                }
                let shape = make_shape(client_id, payload);
                inner.document.shapes.push(shape);
                bump_revision(&mut inner);
                Some(annotation_message(&inner, "add"))
            }
            "annotation.undo" => {
                let index = inner.document.shapes.iter().rposition(|shape| {
                    shape.owner_client_id == client_id && shape.kind != AnnotationKind::Laser
                });
                if let Some(index) = index {
                    inner.document.shapes.remove(index);
                    bump_revision(&mut inner);
                    Some(annotation_message(&inner, "undo"))
                } else {
                    None
                }
            }
            "annotation.clear_own" => {
                let before = inner.document.shapes.len();
                inner
                    .document
                    .shapes
                    .retain(|shape| shape.owner_client_id != client_id);
                if inner.document.shapes.len() != before {
                    bump_revision(&mut inner);
                    Some(annotation_message(&inner, "clear_own"))
                } else {
                    None
                }
            }
            "view.freeze" => {
                let frame = inner.latest_frame.clone().ok_or_else(|| {
                    ProtocolError::new("frame_unavailable", "no captured frame is available yet")
                })?;
                if inner.document.mode != ViewMode::Frozen {
                    inner.frozen_frame = Some(frame.clone());
                    inner.document.mode = ViewMode::Frozen;
                    inner.document.frozen_frame_id = Some(frame.frame_id);
                    bump_revision(&mut inner);
                }
                Some(view_message(&inner))
            }
            "view.resume" => {
                if inner.document.mode != ViewMode::Live {
                    inner.frozen_frame = None;
                    inner.document.mode = ViewMode::Live;
                    inner.document.frozen_frame_id = None;
                    // A resumed live stream invalidates persistent marks; laser
                    // points are ephemeral and may remain until their TTL.
                    inner
                        .document
                        .shapes
                        .retain(|shape| shape.kind == AnnotationKind::Laser);
                    bump_revision(&mut inner);
                }
                Some(view_message(&inner))
            }
            _ => {
                return Err(ProtocolError::new(
                    "unknown_message_type",
                    format!("unsupported interaction message: {message_type}"),
                ));
            }
        };

        if let Some(message) = result.clone() {
            let _ = self.events.send(message);
        }
        Ok(result)
    }

    /// Clear all annotations from the host UI. This is intentionally separate
    /// from client messages so a future Tauri command can call it directly.
    pub fn clear_all(&self) -> ServerEnvelope {
        let mut inner = self.inner.lock().expect("interaction state lock poisoned");
        if !inner.document.shapes.is_empty() {
            inner.document.shapes.clear();
            bump_revision(&mut inner);
        }
        let message = annotation_message(&inner, "clear_all");
        let _ = self.events.send(message.clone());
        message
    }

    /// Advance the capture-source epoch and invalidate annotations/freeze.
    pub fn bump_source_epoch(&self) -> ServerEnvelope {
        let mut inner = self.inner.lock().expect("interaction state lock poisoned");
        inner.document.source_epoch = inner.document.source_epoch.saturating_add(1).max(1);
        inner.document.mode = ViewMode::Live;
        inner.document.frozen_frame_id = None;
        inner.document.shapes.clear();
        // The previous JPEG belongs to the old coordinate space. Keep no
        // stale frame eligible for a new shared freeze while capture recovers.
        inner.latest_frame = None;
        inner.frozen_frame = None;
        bump_revision(&mut inner);
        let message = ServerEnvelope::new(
            "source.changed",
            inner.document.session_id,
            inner.document.source_epoch,
            Some(inner.document.revision),
            Some(serde_json::json!({ "document": inner.document })),
        );
        let _ = self.events.send(message.clone());
        message
    }

    /// Store the most recent encoded frame. The frame is shared with the
    /// existing MJPEG broadcaster, so this does not introduce another copy.
    #[cfg(test)]
    pub fn record_frame(&self, bytes: Arc<Bytes>) -> u64 {
        self.record_frame_with_metadata(bytes, 0, 0)
    }

    pub fn record_frame_with_metadata(&self, bytes: Arc<Bytes>, width: u32, height: u32) -> u64 {
        let mut inner = self.inner.lock().expect("interaction state lock poisoned");
        inner.next_frame_id = inner.next_frame_id.saturating_add(1).max(1);
        let frame_id = inner.next_frame_id;
        inner.latest_frame = Some(StoredFrame {
            frame_id,
            source_epoch: inner.document.source_epoch,
            captured_at_ms: now_ms(),
            width,
            height,
            bytes,
        });
        frame_id
    }

    pub fn frozen_frame(&self, frame_id: u64) -> Option<Arc<Bytes>> {
        let inner = self.inner.lock().ok()?;
        inner
            .frozen_frame
            .as_ref()
            .filter(|frame| frame.frame_id == frame_id)
            .map(|frame| frame.bytes.clone())
    }

    /// Return the newest encoded frame for a one-shot viewer request. The
    /// capture loop already stores this `Arc`, so rate-limited viewers do not
    /// trigger another JPEG encode or wait for a changed screen.
    pub fn latest_frame_bytes(&self) -> Option<Arc<Bytes>> {
        self.inner
            .lock()
            .ok()
            .and_then(|inner| inner.latest_frame.as_ref().map(|frame| frame.bytes.clone()))
    }

    pub fn latest_frame_info(&self) -> Option<FrameInfo> {
        self.inner.lock().ok().and_then(|inner| {
            inner.latest_frame.as_ref().map(|frame| FrameInfo {
                frame_id: frame.frame_id,
                source_epoch: frame.source_epoch,
                captured_at_ms: frame.captured_at_ms,
                width: frame.width,
                height: frame.height,
            })
        })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FrameInfo {
    pub frame_id: u64,
    pub source_epoch: u64,
    pub captured_at_ms: u64,
    pub width: u32,
    pub height: u32,
}

fn validate_client_id(client_id: &str) -> Result<(), ProtocolError> {
    if client_id.is_empty() || client_id.len() > MAX_CLIENT_ID_BYTES {
        return Err(ProtocolError::new(
            "invalid_client_id",
            "client_id is empty or too long",
        ));
    }
    Ok(())
}

fn validate_client_locked(inner: &InteractionInner, client_id: &str) -> Result<(), ProtocolError> {
    validate_client_id(client_id)?;
    if !inner.clients.contains(client_id) {
        return Err(ProtocolError::new(
            "client_not_registered",
            "interaction client is not registered",
        ));
    }
    Ok(())
}

fn parse_payload<T: for<'de> Deserialize<'de>>(
    envelope: &ClientEnvelope,
) -> Result<T, ProtocolError> {
    let payload = envelope.payload.clone().unwrap_or(serde_json::Value::Null);
    serde_json::from_value(payload).map_err(|error| {
        ProtocolError::new(
            "invalid_payload",
            format!("invalid message payload: {error}"),
        )
    })
}

fn validate_add_payload(payload: &AnnotationAddPayload) -> Result<(), ProtocolError> {
    if payload.points.is_empty() || payload.points.len() > MAX_POINTS {
        return Err(ProtocolError::new(
            "invalid_points",
            format!("points must contain 1..={MAX_POINTS} entries"),
        ));
    }
    let required_points = match payload.kind {
        AnnotationKind::Laser => 1,
        AnnotationKind::Arrow | AnnotationKind::Rect => 2,
    };
    if payload.points.len() != required_points {
        return Err(ProtocolError::new(
            "invalid_points",
            format!("{:?} requires {required_points} points", payload.kind),
        ));
    }
    if payload.points.iter().any(|point| {
        !point.x.is_finite()
            || !point.y.is_finite()
            || !(0.0..=1.0).contains(&point.x)
            || !(0.0..=1.0).contains(&point.y)
    }) {
        return Err(ProtocolError::new(
            "invalid_coordinates",
            "annotation coordinates must be finite and within [0, 1]",
        ));
    }
    if !payload.width.is_finite() || !(1.0..=16.0).contains(&payload.width) {
        return Err(ProtocolError::new(
            "invalid_width",
            "annotation width must be between 1 and 16",
        ));
    }
    if !is_hex_color(&payload.color) {
        return Err(ProtocolError::new(
            "invalid_color",
            "annotation color must be a #RRGGBB value",
        ));
    }
    Ok(())
}

fn is_hex_color(color: &str) -> bool {
    color.len() == 7
        && color.as_bytes().first() == Some(&b'#')
        && color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
}

fn make_shape(client_id: &str, payload: AnnotationAddPayload) -> AnnotationShape {
    let expires_at_ms =
        (payload.kind == AnnotationKind::Laser).then(|| now_ms().saturating_add(LASER_TTL_MS));
    AnnotationShape {
        id: Uuid::new_v4().to_string(),
        owner_client_id: client_id.to_string(),
        kind: payload.kind,
        points: payload.points,
        color: payload.color,
        width: payload.width,
        expires_at_ms,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn expire_lasers_locked(inner: &mut InteractionInner) {
    let now = now_ms();
    inner.document.shapes.retain(|shape| {
        shape
            .expires_at_ms
            .map(|expires_at| expires_at > now)
            .unwrap_or(true)
    });
    // Laser points are ephemeral. They intentionally do not advance the
    // document revision because expiration is not a user operation and no
    // dedicated removal event is sent to every client.
}

fn bump_revision(inner: &mut InteractionInner) {
    inner.document.revision = inner.document.revision.saturating_add(1);
}

fn annotation_message(inner: &InteractionInner, operation: &str) -> ServerEnvelope {
    let document = inner.document.clone();
    ServerEnvelope::new(
        "annotation.applied",
        document.session_id,
        document.source_epoch,
        Some(document.revision),
        Some(
            serde_json::to_value(AnnotationAppliedPayload {
                operation: operation.to_string(),
                document,
            })
            .expect("annotation payload is serializable"),
        ),
    )
}

fn view_message(inner: &InteractionInner) -> ServerEnvelope {
    let document = inner.document.clone();
    let snapshot_url = document
        .frozen_frame_id
        .map(|frame_id| format!("/snapshot/{frame_id}"));
    ServerEnvelope::new(
        "view.state",
        document.session_id,
        document.source_epoch,
        Some(document.revision),
        Some(
            serde_json::to_value(ViewStatePayload {
                document,
                snapshot_url,
            })
            .expect("view payload is serializable"),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Arc<InteractionState> {
        InteractionState::new(42)
    }

    fn register(state: &InteractionState, client: &str) {
        state.register_client(client).expect("register client");
    }

    fn envelope(
        message_type: &str,
        session_id: u64,
        source_epoch: u64,
        payload: Option<serde_json::Value>,
    ) -> ClientEnvelope {
        ClientEnvelope {
            v: PROTOCOL_VERSION,
            message_type: message_type.to_string(),
            session_id,
            source_epoch,
            client_seq: None,
            revision: None,
            payload,
        }
    }

    fn add_payload(kind: AnnotationKind) -> serde_json::Value {
        serde_json::json!({
            "kind": kind,
            "points": if kind == AnnotationKind::Laser {
                serde_json::json!([{ "x": 0.2, "y": 0.3 }])
            } else {
                serde_json::json!([{ "x": 0.2, "y": 0.3 }, { "x": 0.8, "y": 0.9 }])
            },
            "color": "#ef4444",
            "width": 4.0
        })
    }

    #[test]
    fn add_is_server_authoritative_and_broadcasts_snapshot_document() {
        let state = state();
        register(&state, "client-a");
        let mut events = state.subscribe();
        let result = state
            .process(
                "client-a",
                envelope(
                    "annotation.add",
                    42,
                    1,
                    Some(add_payload(AnnotationKind::Arrow)),
                ),
            )
            .expect("add succeeds")
            .expect("event expected");
        assert_eq!(result.message_type, "annotation.applied");
        assert_eq!(state.snapshot().shapes.len(), 1);
        assert_eq!(
            events.try_recv().expect("broadcast event").message_type,
            "annotation.applied"
        );
        let json = serde_json::to_value(result).expect("serialize event");
        assert_eq!(json["payload"]["document"]["revision"], 1);
        assert_eq!(
            json["payload"]["document"]["shapes"][0]["owner_client_id"],
            "client-a"
        );
    }

    #[test]
    fn undo_and_clear_only_affect_the_calling_client() {
        let state = state();
        register(&state, "a");
        register(&state, "b");
        for client in ["a", "b"] {
            state
                .process(
                    client,
                    envelope(
                        "annotation.add",
                        42,
                        1,
                        Some(add_payload(AnnotationKind::Arrow)),
                    ),
                )
                .unwrap();
        }
        state
            .process("a", envelope("annotation.undo", 42, 1, None))
            .unwrap();
        let doc = state.snapshot();
        assert_eq!(doc.shapes.len(), 1);
        assert_eq!(doc.shapes[0].owner_client_id, "b");
        state
            .process("b", envelope("annotation.clear_own", 42, 1, None))
            .unwrap();
        assert!(state.snapshot().shapes.is_empty());
    }

    #[test]
    fn rejects_stale_session_epoch_client_and_coordinates() {
        let state = state();
        register(&state, "a");
        let error = state
            .process("a", envelope("annotation.undo", 99, 1, None))
            .unwrap_err();
        assert_eq!(error.code, "session_mismatch");
        let error = state
            .process("a", envelope("annotation.undo", 42, 9, None))
            .unwrap_err();
        assert_eq!(error.code, "source_epoch_mismatch");
        let error = state
            .process("unknown", envelope("annotation.undo", 42, 1, None))
            .unwrap_err();
        assert_eq!(error.code, "client_not_registered");
        let bad = serde_json::json!({
            "kind": "laser",
            "points": [{ "x": 2.0, "y": 0.5 }],
            "color": "#ef4444",
            "width": 4
        });
        let error = state
            .process("a", envelope("annotation.add", 42, 1, Some(bad)))
            .unwrap_err();
        assert_eq!(error.code, "invalid_coordinates");
    }

    #[test]
    fn freeze_requires_a_frame_and_snapshot_route_identity_is_stable() {
        let state = state();
        register(&state, "a");
        let error = state
            .process("a", envelope("view.freeze", 42, 1, None))
            .unwrap_err();
        assert_eq!(error.code, "frame_unavailable");
        let frame_id = state.record_frame(Arc::new(Bytes::from_static(b"jpeg")));
        assert_eq!(frame_id, 1);
        assert_eq!(
            state.latest_frame_bytes().unwrap().as_ref().as_ref(),
            b"jpeg"
        );
        let event = state
            .process("a", envelope("view.freeze", 42, 1, None))
            .unwrap()
            .unwrap();
        assert_eq!(state.snapshot().mode, ViewMode::Frozen);
        assert_eq!(state.snapshot().frozen_frame_id, Some(1));
        assert_eq!(state.frozen_frame(1).unwrap().as_ref().as_ref(), b"jpeg");
        assert!(state.frozen_frame(2).is_none());
        assert_eq!(event.message_type, "view.state");
        state
            .process("a", envelope("view.resume", 42, 1, None))
            .unwrap();
        assert_eq!(state.snapshot().mode, ViewMode::Live);
        assert!(state.frozen_frame(1).is_none());
    }

    #[test]
    fn bounds_and_client_sequence_are_enforced() {
        let state = state();
        register(&state, "a");
        let mut first = envelope("annotation.undo", 42, 1, None);
        first.client_seq = Some(2);
        state.process("a", first).unwrap();
        let mut duplicate = envelope("annotation.undo", 42, 1, None);
        duplicate.client_seq = Some(2);
        let error = state.process("a", duplicate).unwrap_err();
        assert_eq!(error.code, "stale_client_seq");
        let mut too_many_points = Vec::new();
        for _ in 0..=MAX_POINTS {
            too_many_points.push(serde_json::json!({"x": 0.5, "y": 0.5}));
        }
        let bad = serde_json::json!({
            "kind": "arrow",
            "points": too_many_points,
            "color": "#ef4444",
            "width": 4
        });
        let error = state
            .process("a", envelope("annotation.add", 42, 1, Some(bad)))
            .unwrap_err();
        assert_eq!(error.code, "invalid_points");
    }

    #[test]
    fn expired_lasers_do_not_create_revision_gaps() {
        let state = state();
        register(&state, "a");
        state
            .process(
                "a",
                envelope(
                    "annotation.add",
                    42,
                    1,
                    Some(add_payload(AnnotationKind::Laser)),
                ),
            )
            .unwrap();
        {
            let mut inner = state.inner.lock().unwrap();
            inner.document.shapes[0].expires_at_ms = Some(0);
        }
        let document = state.snapshot();
        assert!(document.shapes.is_empty());
        assert_eq!(document.revision, 1);

        let event = state
            .process(
                "a",
                envelope(
                    "annotation.add",
                    42,
                    1,
                    Some(add_payload(AnnotationKind::Rect)),
                ),
            )
            .unwrap()
            .unwrap();
        assert_eq!(event.revision, Some(2));
    }

    #[test]
    fn source_epoch_change_clears_document_and_freeze() {
        let state = state();
        register(&state, "a");
        state.record_frame(Arc::new(Bytes::from_static(b"jpeg")));
        state
            .process(
                "a",
                envelope(
                    "annotation.add",
                    42,
                    1,
                    Some(add_payload(AnnotationKind::Arrow)),
                ),
            )
            .unwrap();
        state
            .process("a", envelope("view.freeze", 42, 1, None))
            .unwrap();
        let event = state.bump_source_epoch();
        assert_eq!(event.message_type, "source.changed");
        let document = state.snapshot();
        assert_eq!(document.source_epoch, 2);
        assert_eq!(document.mode, ViewMode::Live);
        assert!(document.shapes.is_empty());
        assert!(state.frozen_frame(1).is_none());
    }
}
