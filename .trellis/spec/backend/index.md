# Backend Development Guidelines

> Backend code-specs for this project. Add concrete, executable contracts here when a feature crosses Tauri, HTTP, Redis, or other backend boundaries.

---

## Guidelines Index

| Guide | Description | Status |
|-------|-------------|--------|
| [Disk Cache Cleanup](./disk-cache-cleanup.md) | Contracts for Linux local disk, Windows raw disk, IPSAN, and Redis cache-key operations | Active |
| [Clipboard Preview Windows](./clipboard-preview-windows.md) | Contracts for non-activating Alt+C hover preview windows and no-overlap placement | Active |

---

## Notes

- Prefer feature-specific contracts over vague principles.
- For cross-layer features, document request/response shapes, key rules, validation, and test points here.
