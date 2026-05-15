# Backend Development Guidelines

> Backend code-specs for this project. Add concrete, executable contracts here when a feature crosses Tauri, HTTP, Redis, or other backend boundaries.

---

## Guidelines Index

| Guide | Description | Status |
|-------|-------------|--------|
| [Appliance SSH](./appliance-ssh.md) | Contracts for appliance SSH API version port selection, status polling, and enable calls | Active |
| [Disk Cache Cleanup](./disk-cache-cleanup.md) | Contracts for Linux local disk, Windows raw disk, IPSAN, and Redis cache-key operations | Active |
| [Clipboard Preview Windows](./clipboard-preview-windows.md) | Contracts for non-activating Alt+C hover preview windows and no-overlap placement | Active |
| [Error Code Lookup](./error-code-lookup.md) | Contracts for GitLab archive sync, on-disk cache, and Tauri query commands backing the error-code tool | Active |
| [Network Tools](./network-tools.md) | Contracts for streaming port scan events, cancellation, and full TCP port-range support | Active |
| [Screen Share](./screen-share.md) | Contracts for capture startup fencing, stale worker invalidation, and recovery timing | Active |
| [Update Checker](./update-checker.md) | Contracts for manifest fetch, verified update download/apply flow, updater events, and config migration | Active |
| [Tauri Native Dialogs](./tauri-native-dialogs.md) | Contracts for Windows-safe native folder picker execution through the Tauri main thread | Active |

---

## Notes

- Prefer feature-specific contracts over vague principles.
- For cross-layer features, document request/response shapes, key rules, validation, and test points here.
