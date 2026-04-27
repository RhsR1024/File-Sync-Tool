# Release Server

Serve `manifest.json` and versioned `*.exe` files over plain HTTP on the LAN.

Detailed deployment and rollout guide:

- [UPDATE_DEPLOYMENT_GUIDE.md](./UPDATE_DEPLOYMENT_GUIDE.md)

## Quick start

```bash
cd /opt/file-sync-tool-releases
python3 serve.py 8080
```

The app will then fetch `http://<host>:8080/manifest.json`.

## systemd example

```ini
[Unit]
Description=File Sync Tool Release Server
After=network.target

[Service]
WorkingDirectory=/opt/file-sync-tool-releases
ExecStart=/usr/bin/python3 /opt/file-sync-tool-releases/serve.py 8080
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

```bash
systemctl enable --now file-sync-tool-releases
```

## Publish a new release

1. Build on Windows: `pnpm tauri:build:versioned-exe`
2. The command also creates or updates `scripts/release-server/manifest.json`
3. Fill in the new entry's `changelog`
4. Copy the generated `.exe` and `manifest.json` into the release directory

Behavior notes:

- `pnpm tauri:build:versioned-exe` is a project custom command.
- `manifest.json` is updated incrementally by `version`.
- Rebuilding the same version refreshes `url`, `sha256`, and `released_at`, and preserves the existing `changelog`.
- If `manifest.json` is malformed, the script fails instead of overwriting history.

No service restart is required.
