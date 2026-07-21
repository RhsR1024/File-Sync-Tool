//! Serialized Windows input injection for an approved screen-share controller.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct NormalizedInputPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbsolutePoint {
    pub dx: i32,
    pub dy: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMappingError {
    InvalidPoint,
    InvalidScreenRect,
    PointOutsideVirtualDesktop,
}

pub fn normalized_to_physical(
    point: NormalizedInputPoint,
    source: ScreenRect,
) -> Result<PhysicalPoint, InputMappingError> {
    if !point.x.is_finite()
        || !point.y.is_finite()
        || !(0.0..=1.0).contains(&point.x)
        || !(0.0..=1.0).contains(&point.y)
    {
        return Err(InputMappingError::InvalidPoint);
    }
    if source.width == 0 || source.height == 0 {
        return Err(InputMappingError::InvalidScreenRect);
    }

    let x_offset = (point.x * f64::from(source.width.saturating_sub(1))).round() as i64;
    let y_offset = (point.y * f64::from(source.height.saturating_sub(1))).round() as i64;
    let x = i64::from(source.left).saturating_add(x_offset);
    let y = i64::from(source.top).saturating_add(y_offset);
    if x < i64::from(i32::MIN)
        || x > i64::from(i32::MAX)
        || y < i64::from(i32::MIN)
        || y > i64::from(i32::MAX)
    {
        return Err(InputMappingError::InvalidScreenRect);
    }
    Ok(PhysicalPoint {
        x: x as i32,
        y: y as i32,
    })
}

pub fn physical_to_absolute(
    point: PhysicalPoint,
    desktop: ScreenRect,
) -> Result<AbsolutePoint, InputMappingError> {
    if desktop.width == 0 || desktop.height == 0 {
        return Err(InputMappingError::InvalidScreenRect);
    }
    let right = i64::from(desktop.left) + i64::from(desktop.width) - 1;
    let bottom = i64::from(desktop.top) + i64::from(desktop.height) - 1;
    if i64::from(point.x) < i64::from(desktop.left)
        || i64::from(point.x) > right
        || i64::from(point.y) < i64::from(desktop.top)
        || i64::from(point.y) > bottom
    {
        return Err(InputMappingError::PointOutsideVirtualDesktop);
    }

    let x_range = desktop.width.saturating_sub(1).max(1) as i64;
    let y_range = desktop.height.saturating_sub(1).max(1) as i64;
    let dx = ((i64::from(point.x) - i64::from(desktop.left)) * 65_535 + x_range / 2) / x_range;
    let dy = ((i64::from(point.y) - i64::from(desktop.top)) * 65_535 + y_range / 2) / y_range;
    Ok(AbsolutePoint {
        dx: dx.clamp(0, 65_535) as i32,
        dy: dy.clamp(0, 65_535) as i32,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputContext {
    pub client_id: String,
    pub session_id: u64,
    pub source_epoch: u64,
}

impl InputContext {
    pub fn new(client_id: impl Into<String>, session_id: u64, source_epoch: u64) -> Self {
        Self {
            client_id: client_id.into(),
            session_id,
            source_epoch,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyboardKey {
    Letter(u8),
    Digit(u8),
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Enter,
    Escape,
    Backspace,
    Tab,
    Space,
    ControlLeft,
    ControlRight,
    ShiftLeft,
    ShiftRight,
    AltLeft,
    AltRight,
}

impl KeyboardKey {
    pub fn parse(code: &str) -> Result<Self, String> {
        let key = if let Some(letter) = code.strip_prefix("Key") {
            let bytes = letter.as_bytes();
            if bytes.len() == 1 && bytes[0].is_ascii_uppercase() {
                Self::Letter(bytes[0] - b'A')
            } else {
                return Err("unsupported keyboard code".to_string());
            }
        } else if let Some(digit) = code.strip_prefix("Digit") {
            let bytes = digit.as_bytes();
            if bytes.len() == 1 && bytes[0].is_ascii_digit() {
                Self::Digit(bytes[0] - b'0')
            } else {
                return Err("unsupported keyboard code".to_string());
            }
        } else {
            match code {
                "ArrowLeft" => Self::ArrowLeft,
                "ArrowRight" => Self::ArrowRight,
                "ArrowUp" => Self::ArrowUp,
                "ArrowDown" => Self::ArrowDown,
                "Enter" => Self::Enter,
                "Escape" => Self::Escape,
                "Backspace" => Self::Backspace,
                "Tab" => Self::Tab,
                "Space" => Self::Space,
                "ControlLeft" => Self::ControlLeft,
                "ControlRight" => Self::ControlRight,
                "ShiftLeft" => Self::ShiftLeft,
                "ShiftRight" => Self::ShiftRight,
                "AltLeft" => Self::AltLeft,
                "AltRight" => Self::AltRight,
                _ => return Err("unsupported keyboard code".to_string()),
            }
        };
        Ok(key)
    }

    fn is_control_modifier(self) -> bool {
        matches!(self, Self::ControlLeft | Self::ControlRight)
    }

    fn is_alt_modifier(self) -> bool {
        matches!(self, Self::AltLeft | Self::AltRight)
    }

    fn is_modifier(self) -> bool {
        self.is_control_modifier()
            || self.is_alt_modifier()
            || matches!(self, Self::ShiftLeft | Self::ShiftRight)
    }

    fn scan_code(self) -> (u16, bool) {
        const LETTER_SCAN_CODES: [u16; 26] = [
            0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31,
            0x18, 0x19, 0x10, 0x13, 0x1F, 0x14, 0x16, 0x2F, 0x11, 0x2D, 0x15, 0x2C,
        ];
        const DIGIT_SCAN_CODES: [u16; 10] =
            [0x0B, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A];
        match self {
            Self::Letter(value) => (LETTER_SCAN_CODES[usize::from(value)], false),
            Self::Digit(value) => (DIGIT_SCAN_CODES[usize::from(value)], false),
            Self::ArrowLeft => (0x4B, true),
            Self::ArrowRight => (0x4D, true),
            Self::ArrowUp => (0x48, true),
            Self::ArrowDown => (0x50, true),
            Self::Enter => (0x1C, false),
            Self::Escape => (0x01, false),
            Self::Backspace => (0x0E, false),
            Self::Tab => (0x0F, false),
            Self::Space => (0x39, false),
            Self::ControlLeft => (0x1D, false),
            Self::ControlRight => (0x1D, true),
            Self::ShiftLeft => (0x2A, false),
            Self::ShiftRight => (0x36, false),
            Self::AltLeft => (0x38, false),
            Self::AltRight => (0x38, true),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    PointerMove(NormalizedInputPoint),
    PointerButton { button: MouseButton, pressed: bool },
    Wheel { delta_y: i32 },
    Key { key: KeyboardKey, pressed: bool },
    ReleaseAll,
}

#[derive(Debug, Deserialize)]
struct PointerMovePayload {
    x: f64,
    y: f64,
}

#[derive(Debug, Deserialize)]
struct PointerButtonPayload {
    button: MouseButton,
    pressed: bool,
}

#[derive(Debug, Deserialize)]
struct WheelPayload {
    delta_y: i32,
}

#[derive(Debug, Deserialize)]
struct KeyboardPayload {
    code: String,
    pressed: bool,
}

pub fn parse_input_event(
    message_type: &str,
    payload: Option<serde_json::Value>,
) -> Result<InputEvent, String> {
    let payload = payload.unwrap_or(serde_json::Value::Null);
    match message_type {
        "input.pointer_move" => {
            let payload: PointerMovePayload = serde_json::from_value(payload)
                .map_err(|error| format!("invalid pointer move payload: {error}"))?;
            let point = NormalizedInputPoint {
                x: payload.x,
                y: payload.y,
            };
            normalized_to_physical(
                point,
                ScreenRect {
                    left: 0,
                    top: 0,
                    width: 2,
                    height: 2,
                },
            )
            .map_err(|_| "pointer coordinates must be finite and within [0, 1]".to_string())?;
            Ok(InputEvent::PointerMove(point))
        }
        "input.pointer_button" => {
            let payload: PointerButtonPayload = serde_json::from_value(payload)
                .map_err(|error| format!("invalid pointer button payload: {error}"))?;
            Ok(InputEvent::PointerButton {
                button: payload.button,
                pressed: payload.pressed,
            })
        }
        "input.wheel" => {
            let payload: WheelPayload = serde_json::from_value(payload)
                .map_err(|error| format!("invalid wheel payload: {error}"))?;
            if payload.delta_y == 0 || !(-1200..=1200).contains(&payload.delta_y) {
                return Err("wheel delta must be between -1200 and 1200 and not zero".to_string());
            }
            Ok(InputEvent::Wheel {
                delta_y: payload.delta_y,
            })
        }
        "input.key" => {
            if payload.as_object().is_none() {
                return Err("invalid keyboard payload".to_string());
            }
            let payload: KeyboardPayload = serde_json::from_value(payload)
                .map_err(|error| format!("invalid keyboard payload: {error}"))?;
            if payload.code.len() > 32 {
                return Err("keyboard code is too long".to_string());
            }
            let key = KeyboardKey::parse(&payload.code)?;
            Ok(InputEvent::Key {
                key,
                pressed: payload.pressed,
            })
        }
        "input.release_all" => Ok(InputEvent::ReleaseAll),
        _ => Err(format!("unsupported input message: {message_type}")),
    }
}

fn keyboard_combo_allowed(key: KeyboardKey, pressed: &[KeyboardKey]) -> bool {
    let has_alt = pressed.iter().copied().any(KeyboardKey::is_alt_modifier);
    let has_control = pressed
        .iter()
        .copied()
        .any(KeyboardKey::is_control_modifier);
    // Avoid switching applications or invoking system-level escape paths from
    // a browser session. The modifier and ordinary key paths remain useful for
    // common application shortcuts such as Ctrl+C/Ctrl+V.
    if key == KeyboardKey::Tab && has_alt {
        return false;
    }
    if key == KeyboardKey::Escape && (has_alt || has_control) {
        return false;
    }
    true
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueuedInput {
    pub context: InputContext,
    pub event: InputEvent,
}

impl QueuedInput {
    pub fn new(context: InputContext, event: InputEvent) -> Self {
        Self { context, event }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueuePushOutcome {
    Queued,
    Coalesced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputQueueError {
    Full,
}

#[derive(Debug)]
struct PendingInputQueue {
    capacity: usize,
    entries: VecDeque<QueuedInput>,
}

impl PendingInputQueue {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: VecDeque::with_capacity(capacity.max(1)),
        }
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn push(&mut self, input: QueuedInput) -> Result<QueuePushOutcome, InputQueueError> {
        if matches!(input.event, InputEvent::PointerMove(_)) {
            // Only merge adjacent moves. Looking past a button/wheel/key edge
            // would move a newer coordinate ahead of that edge and make clicks
            // land at a position the viewer did not intend.
            if let Some(existing) = self.entries.back_mut().filter(|candidate| {
                matches!(candidate.event, InputEvent::PointerMove(_))
                    && candidate.context == input.context
            }) {
                *existing = input;
                return Ok(QueuePushOutcome::Coalesced);
            }
        }
        if self.entries.len() >= self.capacity {
            return Err(InputQueueError::Full);
        }
        self.entries.push_back(input);
        Ok(QueuePushOutcome::Queued)
    }

    fn pop_front(&mut self) -> Option<QueuedInput> {
        self.entries.pop_front()
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

const INPUT_QUEUE_CAPACITY: usize = 128;

#[derive(Debug, Clone)]
struct ActiveInput {
    context: InputContext,
    source: ScreenRect,
    desktop: ScreenRect,
}

#[derive(Debug)]
struct WorkerState {
    queue: PendingInputQueue,
    active: Option<ActiveInput>,
    release_pending: bool,
    stopping: bool,
    failed: bool,
}

impl WorkerState {
    fn request_release_all(&mut self, context: &InputContext) -> bool {
        if self.stopping
            || self
                .active
                .as_ref()
                .is_none_or(|active| &active.context != context)
        {
            return false;
        }
        self.queue.clear();
        self.release_pending = true;
        true
    }
}

struct WorkerShared {
    state: Mutex<WorkerState>,
    wake: Condvar,
}

/// Handle shared by the HTTP/Tauri layers. All actual OS input calls happen on
/// the worker thread; callers only mutate this bounded state.
pub struct InputWorkerHandle {
    shared: Arc<WorkerShared>,
    failed: Arc<AtomicBool>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for InputWorkerHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InputWorkerHandle")
            .field("failed", &self.failed.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl InputWorkerHandle {
    pub fn spawn() -> Result<Arc<Self>, String> {
        let shared = Arc::new(WorkerShared {
            state: Mutex::new(WorkerState {
                queue: PendingInputQueue::new(INPUT_QUEUE_CAPACITY),
                active: None,
                release_pending: false,
                stopping: false,
                failed: false,
            }),
            wake: Condvar::new(),
        });
        let failed = Arc::new(AtomicBool::new(false));
        let thread_shared = shared.clone();
        let thread_failed = failed.clone();
        let join = thread::Builder::new()
            .name("screen-input-worker".into())
            .spawn(move || worker_loop(thread_shared, thread_failed))
            .map_err(|error| format!("failed to start screen input worker: {error}"))?;
        Ok(Arc::new(Self {
            shared,
            failed,
            join: Mutex::new(Some(join)),
        }))
    }

    pub fn grant(&self, context: InputContext, source: ScreenRect) -> Result<(), String> {
        let desktop = virtual_desktop_rect();
        if desktop.width == 0 || desktop.height == 0 {
            return Err("virtual desktop is unavailable".to_string());
        }
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| "input worker state is unavailable".to_string())?;
        if state.stopping {
            return Err("input worker is stopping".to_string());
        }
        state.active = Some(ActiveInput {
            context,
            source,
            desktop,
        });
        state.queue.clear();
        state.release_pending = true;
        self.shared.wake.notify_one();
        Ok(())
    }

    pub fn revoke(&self) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.active = None;
            state.queue.clear();
            state.release_pending = true;
            self.shared.wake.notify_one();
        }
    }

    pub fn enqueue(&self, input: QueuedInput) -> Result<QueuePushOutcome, InputQueueError> {
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| InputQueueError::Full)?;
        let Some(active) = state.active.as_ref() else {
            return Err(InputQueueError::Full);
        };
        if active.context != input.context || state.stopping {
            return Err(InputQueueError::Full);
        }
        let result = state.queue.push(input);
        if result.is_ok() {
            self.shared.wake.notify_one();
        }
        result
    }

    pub fn release_all(&self, context: &InputContext) -> Result<(), InputQueueError> {
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| InputQueueError::Full)?;
        if !state.request_release_all(context) {
            return Err(InputQueueError::Full);
        }
        self.shared.wake.notify_one();
        Ok(())
    }

    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    pub fn shutdown(&self) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.stopping = true;
            state.active = None;
            state.queue.clear();
            state.release_pending = true;
            self.shared.wake.notify_one();
        }
        if let Ok(mut join) = self.join.lock() {
            if let Some(thread) = join.take() {
                let _ = thread.join();
            }
        }
    }
}

impl Drop for InputWorkerHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn worker_loop(shared: Arc<WorkerShared>, failed: Arc<AtomicBool>) {
    let mut sink = InputSink::new();
    loop {
        let (input, active, release) = {
            let mut state = match shared.state.lock() {
                Ok(state) => state,
                Err(_) => {
                    failed.store(true, Ordering::Relaxed);
                    return;
                }
            };
            while !state.stopping && state.queue.entries.is_empty() && !state.release_pending {
                state = match shared.wake.wait(state) {
                    Ok(state) => state,
                    Err(_) => {
                        failed.store(true, Ordering::Relaxed);
                        return;
                    }
                };
            }
            let release = state.release_pending;
            state.release_pending = false;
            let input = state.queue.pop_front();
            let active = state.active.clone();
            let stopping = state.stopping;
            (input, active, release || stopping)
        };

        if release && sink.release_all().is_err() {
            failed.store(true, Ordering::Relaxed);
            if let Ok(mut state) = shared.state.lock() {
                state.failed = true;
                state.active = None;
                state.queue.clear();
            }
            if let Ok(state) = shared.state.lock() {
                if state.stopping {
                    return;
                }
            }
        }

        if let Some(input) = input {
            let Some(active) = active else { continue };
            if active.context != input.context {
                continue;
            }
            if sink.execute(&input.event, &active).is_err() {
                failed.store(true, Ordering::Relaxed);
                if let Ok(mut state) = shared.state.lock() {
                    state.failed = true;
                    state.active = None;
                    state.queue.clear();
                    state.release_pending = true;
                }
            }
        }

        if let Ok(state) = shared.state.lock() {
            if state.stopping && state.queue.entries.is_empty() && !state.release_pending {
                return;
            }
        }
    }
}

struct InputSink {
    buttons_down: Vec<MouseButton>,
    keys_down: Vec<KeyboardKey>,
}

impl InputSink {
    fn new() -> Self {
        Self {
            buttons_down: Vec::new(),
            keys_down: Vec::new(),
        }
    }

    fn execute(&mut self, event: &InputEvent, active: &ActiveInput) -> Result<(), String> {
        match event {
            InputEvent::PointerMove(point) => {
                let physical = normalized_to_physical(*point, active.source)
                    .map_err(|error| format!("invalid pointer coordinate: {error:?}"))?;
                let absolute = physical_to_absolute(physical, active.desktop)
                    .map_err(|error| format!("pointer is outside virtual desktop: {error:?}"))?;
                self.send_move(absolute)
            }
            InputEvent::PointerButton { button, pressed } => self.send_button(*button, *pressed),
            InputEvent::Wheel { delta_y } => self.send_wheel(*delta_y),
            InputEvent::Key { key, pressed } => self.send_key(*key, *pressed),
            InputEvent::ReleaseAll => self.release_all(),
        }
    }

    fn release_all(&mut self) -> Result<(), String> {
        let mut first_error = None;
        let pressed_buttons = std::mem::take(&mut self.buttons_down);
        for button in pressed_buttons.into_iter().rev() {
            if let Err(error) = self.send_button(button, false) {
                first_error.get_or_insert(error);
            }
        }
        let mut pressed_keys = std::mem::take(&mut self.keys_down);
        pressed_keys.sort_by_key(|key| key.is_modifier());
        for key in pressed_keys {
            if let Err(error) = self.send_key(key, false) {
                first_error.get_or_insert(error);
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }

    fn send_move(&mut self, point: AbsolutePoint) -> Result<(), String> {
        send_mouse_input(point, 0, mouse_move_flags())
    }

    fn send_button(&mut self, button: MouseButton, pressed: bool) -> Result<(), String> {
        let flags = match (button, pressed) {
            (MouseButton::Left, true) => mouse_left_down_flags(),
            (MouseButton::Left, false) => mouse_left_up_flags(),
            (MouseButton::Right, true) => mouse_right_down_flags(),
            (MouseButton::Right, false) => mouse_right_up_flags(),
        };
        send_mouse_input(AbsolutePoint { dx: 0, dy: 0 }, 0, flags)?;
        if pressed {
            if !self.buttons_down.contains(&button) {
                self.buttons_down.push(button);
            }
        } else {
            self.buttons_down.retain(|held| *held != button);
        }
        Ok(())
    }

    fn send_wheel(&mut self, delta_y: i32) -> Result<(), String> {
        send_mouse_input(
            AbsolutePoint { dx: 0, dy: 0 },
            delta_y as u32,
            mouse_wheel_flags(),
        )
    }

    fn send_key(&mut self, key: KeyboardKey, pressed: bool) -> Result<(), String> {
        if pressed && !keyboard_combo_allowed(key, &self.keys_down) {
            return Ok(());
        }
        if !pressed && !self.keys_down.contains(&key) {
            return Ok(());
        }
        let (scan_code, extended) = key.scan_code();
        send_keyboard_input(scan_code, pressed, extended)?;
        if pressed {
            if !self.keys_down.contains(&key) {
                self.keys_down.push(key);
            }
        } else {
            self.keys_down.retain(|held| *held != key);
        }
        Ok(())
    }
}

fn mouse_move_flags() -> u32 {
    0x0001 | 0x8000 | 0x4000
}

fn mouse_left_down_flags() -> u32 {
    0x0002
}

fn mouse_left_up_flags() -> u32 {
    0x0004
}

fn mouse_right_down_flags() -> u32 {
    0x0008
}

fn mouse_right_up_flags() -> u32 {
    0x0010
}

fn mouse_wheel_flags() -> u32 {
    0x0800
}

#[cfg(target_os = "windows")]
fn send_mouse_input(point: AbsolutePoint, data: u32, flags: u32) -> Result<(), String> {
    use std::mem;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEINPUT, MOUSE_EVENT_FLAGS,
    };

    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: point.dx,
                dy: point.dy,
                mouseData: data,
                dwFlags: MOUSE_EVENT_FLAGS(flags),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inserted = unsafe { SendInput(&[input], mem::size_of::<INPUT>() as i32) };
    if inserted == 1 {
        Ok(())
    } else {
        Err(format!(
            "SendInput failed: {}",
            windows::core::Error::from_win32()
        ))
    }
}

#[cfg(not(target_os = "windows"))]
fn send_mouse_input(_point: AbsolutePoint, _data: u32, _flags: u32) -> Result<(), String> {
    Err("remote input is only supported on Windows".to_string())
}

#[cfg(target_os = "windows")]
fn send_keyboard_input(scan_code: u16, pressed: bool, extended: bool) -> Result<(), String> {
    use std::mem;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
        KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, VIRTUAL_KEY,
    };

    let mut flags = KEYEVENTF_SCANCODE;
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if !pressed {
        flags |= KEYEVENTF_KEYUP;
    }
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: scan_code,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inserted = unsafe { SendInput(&[input], mem::size_of::<INPUT>() as i32) };
    if inserted == 1 {
        Ok(())
    } else {
        Err(format!(
            "SendInput failed: {}",
            windows::core::Error::from_win32()
        ))
    }
}

#[cfg(not(target_os = "windows"))]
fn send_keyboard_input(_scan_code: u16, _pressed: bool, _extended: bool) -> Result<(), String> {
    Err("remote input is only supported on Windows".to_string())
}

#[cfg(target_os = "windows")]
fn virtual_desktop_rect() -> ScreenRect {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };
    unsafe {
        ScreenRect {
            left: GetSystemMetrics(SM_XVIRTUALSCREEN),
            top: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN).max(0) as u32,
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN).max(0) as u32,
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn virtual_desktop_rect() -> ScreenRect {
    ScreenRect {
        left: 0,
        top: 0,
        width: 1,
        height: 1,
    }
}

/// Resolve the source monitor in the same primary-first ordering used by the
/// screen capture path. The fallback dimensions are used only when Windows
/// reports an empty rectangle for a transient topology change.
#[cfg(target_os = "windows")]
pub fn source_rect_for_monitor(
    monitor_index: usize,
    fallback_width: u32,
    fallback_height: u32,
) -> Option<ScreenRect> {
    use std::mem;
    use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    };
    const MONITORINFOF_PRIMARY: u32 = 1;

    #[derive(Debug)]
    struct Entry {
        rect: ScreenRect,
        primary: bool,
    }

    unsafe extern "system" fn callback(
        monitor: HMONITOR,
        _dc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let entries = &mut *(data.0 as *mut Vec<Entry>);
        let mut info = MONITORINFO {
            cbSize: mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let width = info
                .rcMonitor
                .right
                .saturating_sub(info.rcMonitor.left)
                .max(0) as u32;
            let height = info
                .rcMonitor
                .bottom
                .saturating_sub(info.rcMonitor.top)
                .max(0) as u32;
            entries.push(Entry {
                rect: ScreenRect {
                    left: info.rcMonitor.left,
                    top: info.rcMonitor.top,
                    width,
                    height,
                },
                primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
            });
        }
        BOOL(1)
    }

    let mut entries: Vec<Entry> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(callback),
            LPARAM(&mut entries as *mut Vec<Entry> as isize),
        );
    }
    entries.sort_by(|left, right| {
        right
            .primary
            .cmp(&left.primary)
            .then(left.rect.left.cmp(&right.rect.left))
            .then(left.rect.top.cmp(&right.rect.top))
    });
    entries.get(monitor_index).map(|entry| ScreenRect {
        width: if entry.rect.width == 0 {
            fallback_width
        } else {
            entry.rect.width
        },
        height: if entry.rect.height == 0 {
            fallback_height
        } else {
            entry.rect.height
        },
        ..entry.rect
    })
}

#[cfg(not(target_os = "windows"))]
pub fn source_rect_for_monitor(
    _monitor_index: usize,
    fallback_width: u32,
    fallback_height: u32,
) -> Option<ScreenRect> {
    Some(ScreenRect {
        left: 0,
        top: 0,
        width: fallback_width,
        height: fallback_height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: ScreenRect = ScreenRect {
        left: 0,
        top: 0,
        width: 1920,
        height: 1080,
    };

    #[test]
    fn normalized_points_map_to_source_monitor_edges() {
        assert_eq!(
            normalized_to_physical(NormalizedInputPoint { x: 0.0, y: 0.0 }, PRIMARY).unwrap(),
            PhysicalPoint { x: 0, y: 0 }
        );
        assert_eq!(
            normalized_to_physical(NormalizedInputPoint { x: 1.0, y: 1.0 }, PRIMARY).unwrap(),
            PhysicalPoint { x: 1919, y: 1079 }
        );
    }

    #[test]
    fn negative_left_monitor_maps_into_left_half_of_virtual_desktop() {
        let source = ScreenRect {
            left: -1920,
            top: 0,
            width: 1920,
            height: 1080,
        };
        let desktop = ScreenRect {
            left: -1920,
            top: 0,
            width: 3840,
            height: 1080,
        };
        let physical =
            normalized_to_physical(NormalizedInputPoint { x: 0.5, y: 0.5 }, source).unwrap();
        let absolute = physical_to_absolute(physical, desktop).unwrap();

        assert!(physical.x < 0);
        assert!(absolute.dx < 32768);
        assert!((32700..=32840).contains(&absolute.dy));
    }

    #[test]
    fn pointer_mapping_rejects_non_finite_out_of_bounds_and_empty_rects() {
        for point in [
            NormalizedInputPoint {
                x: f64::NAN,
                y: 0.5,
            },
            NormalizedInputPoint { x: -0.01, y: 0.5 },
            NormalizedInputPoint { x: 0.5, y: 1.01 },
        ] {
            assert!(normalized_to_physical(point, PRIMARY).is_err());
        }
        assert!(normalized_to_physical(
            NormalizedInputPoint { x: 0.5, y: 0.5 },
            ScreenRect {
                width: 0,
                ..PRIMARY
            },
        )
        .is_err());
    }

    #[test]
    fn pending_moves_are_coalesced_but_edge_events_are_never_silently_dropped() {
        let context = InputContext::new("controller", 7, 3);
        let mut queue = PendingInputQueue::new(2);

        assert_eq!(
            queue.push(QueuedInput::new(
                context.clone(),
                InputEvent::PointerMove(NormalizedInputPoint { x: 0.1, y: 0.1 }),
            )),
            Ok(QueuePushOutcome::Queued)
        );
        assert_eq!(
            queue.push(QueuedInput::new(
                context.clone(),
                InputEvent::PointerMove(NormalizedInputPoint { x: 0.8, y: 0.9 }),
            )),
            Ok(QueuePushOutcome::Coalesced)
        );
        assert_eq!(queue.len(), 1);

        queue
            .push(QueuedInput::new(
                context.clone(),
                InputEvent::PointerButton {
                    button: MouseButton::Left,
                    pressed: true,
                },
            ))
            .unwrap();
        let full = queue
            .push(QueuedInput::new(
                context,
                InputEvent::PointerButton {
                    button: MouseButton::Left,
                    pressed: false,
                },
            ))
            .expect_err("button release must report queue saturation");
        assert_eq!(full, InputQueueError::Full);
    }

    #[test]
    fn pointer_moves_never_coalesce_across_button_edges() {
        let context = InputContext::new("controller", 7, 3);
        let mut queue = PendingInputQueue::new(4);
        queue
            .push(QueuedInput::new(
                context.clone(),
                InputEvent::PointerMove(NormalizedInputPoint { x: 0.1, y: 0.1 }),
            ))
            .unwrap();
        queue
            .push(QueuedInput::new(
                context.clone(),
                InputEvent::PointerButton {
                    button: MouseButton::Left,
                    pressed: true,
                },
            ))
            .unwrap();
        queue
            .push(QueuedInput::new(
                context.clone(),
                InputEvent::PointerMove(NormalizedInputPoint { x: 0.6, y: 0.7 }),
            ))
            .unwrap();
        assert_eq!(
            queue.push(QueuedInput::new(
                context,
                InputEvent::PointerMove(NormalizedInputPoint { x: 0.8, y: 0.9 }),
            )),
            Ok(QueuePushOutcome::Coalesced)
        );

        assert!(matches!(
            queue.pop_front().map(|input| input.event),
            Some(InputEvent::PointerMove(NormalizedInputPoint {
                x: 0.1,
                y: 0.1
            }))
        ));
        assert!(matches!(
            queue.pop_front().map(|input| input.event),
            Some(InputEvent::PointerButton { pressed: true, .. })
        ));
        assert!(matches!(
            queue.pop_front().map(|input| input.event),
            Some(InputEvent::PointerMove(NormalizedInputPoint {
                x: 0.8,
                y: 0.9
            }))
        ));
    }

    #[test]
    fn protocol_payloads_accept_mouse_events_and_reject_invalid_ranges() {
        assert_eq!(
            parse_input_event(
                "input.pointer_move",
                Some(serde_json::json!({ "x": 0.25, "y": 0.75 })),
            )
            .unwrap(),
            InputEvent::PointerMove(NormalizedInputPoint { x: 0.25, y: 0.75 })
        );
        assert!(parse_input_event(
            "input.pointer_move",
            Some(serde_json::json!({ "x": 1.5, "y": 0.5 })),
        )
        .is_err());
        assert_eq!(
            parse_input_event(
                "input.pointer_button",
                Some(serde_json::json!({ "button": "right", "pressed": true })),
            )
            .unwrap(),
            InputEvent::PointerButton {
                button: MouseButton::Right,
                pressed: true,
            }
        );
        assert!(
            parse_input_event("input.wheel", Some(serde_json::json!({ "delta_y": 5000 })),)
                .is_err()
        );
    }

    #[test]
    fn keyboard_protocol_accepts_only_the_documented_key_whitelist() {
        assert_eq!(
            parse_input_event(
                "input.key",
                Some(serde_json::json!({ "code": "KeyA", "pressed": true })),
            )
            .unwrap(),
            InputEvent::Key {
                key: KeyboardKey::Letter(0),
                pressed: true,
            }
        );
        assert_eq!(
            parse_input_event(
                "input.key",
                Some(serde_json::json!({ "code": "Digit9", "pressed": false })),
            )
            .unwrap(),
            InputEvent::Key {
                key: KeyboardKey::Digit(9),
                pressed: false,
            }
        );
        for code in [
            "ArrowLeft",
            "ArrowRight",
            "ArrowUp",
            "ArrowDown",
            "Enter",
            "Escape",
            "Backspace",
            "Tab",
            "Space",
            "ControlLeft",
            "ControlRight",
            "ShiftLeft",
            "ShiftRight",
            "AltLeft",
            "AltRight",
        ] {
            assert!(parse_input_event(
                "input.key",
                Some(serde_json::json!({ "code": code, "pressed": true })),
            )
            .is_ok());
        }
        for code in ["MetaLeft", "F1", "Delete", "Numpad0", "IntlBackslash"] {
            assert!(parse_input_event(
                "input.key",
                Some(serde_json::json!({ "code": code, "pressed": true })),
            )
            .is_err());
        }
        assert_eq!(
            parse_input_event("input.release_all", None).unwrap(),
            InputEvent::ReleaseAll
        );
    }

    #[test]
    fn restricted_system_shortcuts_are_filtered_without_blocking_common_combinations() {
        assert!(!keyboard_combo_allowed(
            KeyboardKey::Tab,
            &[KeyboardKey::AltLeft]
        ));
        assert!(!keyboard_combo_allowed(
            KeyboardKey::Escape,
            &[KeyboardKey::ControlLeft]
        ));
        assert!(keyboard_combo_allowed(
            KeyboardKey::Letter(2),
            &[KeyboardKey::ControlLeft]
        ));
        assert!(keyboard_combo_allowed(
            KeyboardKey::Letter(21),
            &[KeyboardKey::ControlRight, KeyboardKey::ShiftLeft]
        ));
    }

    #[test]
    fn keyboard_scan_code_mapping_is_stable() {
        assert_eq!(KeyboardKey::Letter(0).scan_code(), (0x1E, false));
        assert_eq!(KeyboardKey::Letter(25).scan_code(), (0x2C, false));
        assert_eq!(KeyboardKey::Digit(9).scan_code(), (0x0A, false));
        assert_eq!(KeyboardKey::Digit(0).scan_code(), (0x0B, false));
        assert_eq!(KeyboardKey::ArrowLeft.scan_code(), (0x4B, true));
        assert_eq!(KeyboardKey::ControlRight.scan_code(), (0x1D, true));
        assert_eq!(KeyboardKey::AltLeft.scan_code(), (0x38, false));
    }

    #[test]
    fn release_all_request_clears_edges_without_revoking_the_controller() {
        let context = InputContext::new("controller", 7, 3);
        let mut state = WorkerState {
            queue: PendingInputQueue::new(4),
            active: Some(ActiveInput {
                context: context.clone(),
                source: PRIMARY,
                desktop: PRIMARY,
            }),
            release_pending: false,
            stopping: false,
            failed: false,
        };
        state
            .queue
            .push(QueuedInput::new(
                context.clone(),
                InputEvent::Key {
                    key: KeyboardKey::ControlLeft,
                    pressed: true,
                },
            ))
            .unwrap();

        assert!(state.request_release_all(&context));
        assert_eq!(state.queue.len(), 0);
        assert!(state.release_pending);
        assert!(state.active.is_some());
        assert!(!state.request_release_all(&InputContext::new("other", 7, 3)));
    }
}
