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

## Run in background with nohup

Use this when you want the server to keep running after the current shell exits:

```bash
cd /opt/file-sync-tool-releases
nohup python3 serve.py 8080 > release-server.log 2>&1 &
```

Useful follow-up commands:

```bash
tail -f /opt/file-sync-tool-releases/release-server.log
ps -ef | grep serve.py
```

Note: `nohup` does not automatically start the server again after a reboot.

## systemd example

Recommended for long-term deployment. This keeps the service running after logout
and can start it automatically on reboot.

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
systemctl daemon-reload
systemctl enable --now file-sync-tool-releases
systemctl status file-sync-tool-releases
journalctl -u file-sync-tool-releases -f
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

## WebView2 Runtime assets

The bare `.exe` startup bootstrap downloads the WebView2 Runtime installer from
the update server. These files are independent of `manifest.json`:

```text
<server-root>/
|-- manifest.json                  # app update manifest (existing)
|-- file-sync-tool-*.exe           # app release artifacts (existing)
`-- webview2/
    |-- MicrosoftEdgeWebView2RuntimeInstallerX64.exe
    `-- MicrosoftEdgeWebView2RuntimeInstallerX64.exe.sha256
```

Download the "Evergreen Standalone Installer x64" from
https://developer.microsoft.com/microsoft-edge/webview2/ and generate the
sidecar hash file:

```powershell
$f = "MicrosoftEdgeWebView2RuntimeInstallerX64.exe"
$hash = (Get-FileHash $f -Algorithm SHA256).Hash.ToLower()
"$hash  $f" | Out-File -Encoding ascii "$f.sha256" -NoNewline
```
