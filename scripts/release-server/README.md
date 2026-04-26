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
2. Copy the generated `.exe` into the release directory
3. Prepend a new entry to `manifest.json` and bump `latest`
4. Compute SHA-256 with `sha256sum file-sync-tool-*.exe`

No service restart is required.
