# brainstorm: LAN share connection cleanup and copy recovery

## Goal

Improve LAN screen share, file share, and console copy reliability so stale connection records can be cleared accurately and interrupted remote copies can recover instead of ending in misleading success states.

## Requirements

- Screen share and file share should stop showing stale connected IPs after the remote browser/page is closed.
- The host page should support either automatic refresh, manual refresh, or both, depending on the final design.
- Manual and scheduled copy flows should not report success when any file copy actually fails.
- Copy failures caused by transient network issues such as `os error 64` should retry instead of stopping immediately.
- Partial local files should be resumable or otherwise recoverable without forcing users to delete them manually.
- Scheduled scans should detect same-name local files whose size does not match the remote source and retry the copy until it succeeds.
- Manually triggered copies should follow the same recovery rules as scheduled copies where feasible, including continuing as persistent background recovery work after the initiating page is closed.

## Acceptance Criteria

- Closing a remote browser session eventually removes its stale IP from the screen share and file share connection panels.
- The UI offers an explicit way to refresh connection status if automatic cleanup is delayed.
- A file copy that logs `Failed to copy ...` cannot leave the task run in a completed state.
- Transient remote-copy failures retry for the agreed window and surface retry progress in logs or task state.
- After retry exhaustion, later scan/manual recovery can still detect an incomplete local file by size mismatch and re-attempt the copy.
- Recovery behavior is defined for both scheduled copies and user-initiated manual copies, and manual copies do not depend on the initiating page staying open.

## Technical Notes

- Current `src-tauri/src/fileshare` status tracking stores unique visitor IPs but does not remove them when the client is no longer active.
- Current `src-tauri/src/screenshare.rs` tracks active MJPEG viewers with connection guards, but browser-close cleanup may lag and the file is already dirty in the working tree.
- Current `src-tauri/src/scanner.rs` directory copy flow logs per-file failures without bubbling them up, so task status can be marked complete despite copy errors.
- Current size-mismatch re-copy logic is gated by persisted task records, which may block desired re-copy behavior for incomplete files.
