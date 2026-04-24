# Clipboard Preview Windows

> Contracts for the standalone Alt+C clipboard hover preview windows.

## Scenario: Non-Activating Hover Preview

### 1. Scope / Trigger

- Trigger: Code that creates, shows, positions, hides, or resizes `clipboard-image-preview` or `clipboard-text-preview`.
- Goal: Hover previews must show image/text payloads without stealing focus from `clipboard-panel` and without covering the panel's controls.

### 2. Signatures

- Rust module: `src-tauri/src/clipboard/preview.rs`
- Window labels:
  - `clipboard-image-preview`
  - `clipboard-text-preview`
- Public commands:
  - `cb_show_image_preview(id: i64, token: Option<u64>) -> Result<(), String>`
  - `cb_show_text_preview(id: i64, token: Option<u64>) -> Result<(), String>`
  - `cb_hide_preview(token: Option<u64>)`
  - `cb_get_image_preview_payload() -> Option<ImagePreviewPayload>`
  - `cb_get_text_preview_payload() -> Option<TextPreviewPayload>`
- Placement helper:
  - `calculate_preview_placement(panel_rect, preview_size, monitor_rect, preference) -> PreviewPlacement`

### 3. Contracts

- Preview windows must be built with `.focusable(false)`, `.focused(false)`, `.skip_taskbar(true)`, `.always_on_top(true)`, and `.visible(false)`.
- Showing a preview on Windows must use `SWP_NOACTIVATE` after `window.show()` so the Alt+C panel keeps focus.
- Preview windows must call `set_ignore_cursor_events(true)` before showing so mouse clicks and drag attempts continue to reach the panel.
- Windows preview HWNDs and their WebView child HWNDs must also be forced to `WS_EX_TRANSPARENT | WS_EX_NOACTIVATE` with `SetWindowLongW`, followed by `SetWindowPos(... SWP_FRAMECHANGED ...)`. Apply this before and after `show()`, because WebView2/window display can recreate or reset hit-test behavior.
- Preview windows must be inserted behind the `clipboard-panel` HWND in z-order when shown. They may be topmost as a group, but the panel must remain above them so overlap cannot block titlebar, settings, lock, close, or list clicks.
- Preview HTML pages must be passive display surfaces (`pointer-events: none`, no `cursor: pointer`). They must not own clicks, drags, wheel, or hover affordances that belong to the Alt+C panel.
- Clipboard rows that can trigger hover previews must emit a leave event from the row itself, not only from the scroll container.
- The Alt+C panel shell must hide previews during pointerdown capture, and header/control hover must also hide previews before settings, lock, close, or drag handlers run.
- Hover preview show/hide requests must be token guarded across frontend and Rust command boundaries. A delayed or in-flight show request must not display a preview after the matching hover token has been hidden or replaced.
- Placement must use real side space next to the panel:
  - Left side max width: `panel.x - monitor.x - PREVIEW_GAP_PX`
  - Right side max width: `monitor.right - panel.right - PREVIEW_GAP_PX`
- If the desired preview width is larger than the chosen side, shrink the preview width to that side's available width instead of clamping the x-position back over the panel.

### 4. Validation & Error Matrix

| Case | Expected Behavior |
| --- | --- |
| Image item has no `image_path` | Return an error before opening a preview window. |
| Text item has empty full/preview content | Hide preview windows and return `Ok(())`. |
| Preview disabled in settings | Clear cached payloads, hide preview windows, return `Ok(())`. |
| Preferred side cannot fit the minimum width but the other side can | Flip to the other side. |
| Preferred side has usable space but not full desired width | Keep the side and shrink width to avoid panel overlap. |
| Preview window already exists | Reuse it, reapply non-focusable/cursor-ignore behavior before showing. |
| Mouse leaves a previewable row for the header or toolbar | Hide the preview before panel controls process pointer input. |
| Hover leaves while a show command is still in-flight | The stale show token is rejected or immediately hidden; it must not resurrect the preview. |
| A newer row hover starts before an older hide completes | The older hide token must not hide the newer preview. |
| Preview visually overlaps the panel | The panel must still receive clicks/drags due native click-through styles and passive HTML. |
| Native click-through is inconsistent for a WebView2 child window | The panel remains above the preview in z-order, so overlap still cannot intercept panel controls. |

### 5. Good/Base/Bad Cases

- Good: A 960px image preview beside a 420px panel on a 1920px monitor shrinks to the right-side space and starts at `panel.right + PREVIEW_GAP_PX`.
- Base: A 360px preview with enough requested-side space keeps the requested side and width.
- Bad: A preview larger than the side space is clamped to `monitor.right - width`; this can overlap the panel and make settings, lock, close, and dragging appear frozen.

### 6. Tests Required

- Rust: `cargo test clipboard::preview`
  - Assert requested-side placement still works.
  - Assert right-overflow flips left when the requested side has almost no space.
  - Assert a wide preview shrinks to side space without overlapping the panel.
  - Assert cached payload retrieval survives lazy preview window creation.
  - Assert canceled preview tokens reject stale show requests.
  - Assert older hide tokens do not cancel newer hover previews.
- Node: `node --test src/pages/ClipboardPreviewPage.test.mjs`
  - Assert preview backend uses standalone HTML files.
  - Assert preview windows ignore cursor events.
  - Assert preview windows enforce native Windows click-through styles after showing.
  - Assert preview windows are shown behind the Alt+C panel in z-order.
  - Assert preview windows are non-focusable and shown through the non-activating helper.
  - Assert preview HTML remains passive.
  - Assert frontend API, Tauri commands, and backend preview logic pass and validate hover tokens.
- Node: `node --test src/pages/ClipboardPanelPage.test.mjs`
  - Assert list rows emit hover leave.
  - Assert pinned lists forward hover leave.
  - Assert the panel hides previews before control pointer handlers run.

### 7. Wrong vs Correct

#### Wrong

```rust
let x = unclamped_x.clamp(monitor_rect.x, monitor_rect.right() - width as i32);
window.show()?;
```

This keeps the preview on screen, but can move it back over `clipboard-panel` and activate the preview window.

#### Correct

```rust
let width = preview_size.width.min(side_available.max(1));
let x = panel_rect.right() + PREVIEW_GAP_PX;
show_preview_without_focus(&window, &panel)?;
```

This preserves the side gap, keeps the preview below the panel in z-order, and keeps focus on the Alt+C panel.
