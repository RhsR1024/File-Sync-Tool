# File Sync Tool 升级服务器

本目录用于在局域网或内网中提供 File Sync Tool 程序升级包、WebView2 运行时，以及 Notepad++ 插件目录。服务器只需要通过 HTTP 提供静态文件。

完整部署和升级实施说明：

- [更新部署实施手册](./UPDATE_DEPLOYMENT_GUIDE.md)
- [Notepad++ 插件目录说明](./notepad-plugins/README.md)

## 快速启动

```bash
cd /opt/file-sync-tool-releases
python3 serve.py 8080
```

启动后，客户端将从以下地址读取程序升级清单：

```text
http://<服务器地址>:8080/manifest.json
```

## 使用 nohup 后台运行

如果需要在退出当前终端后继续运行服务器，可以使用：

```bash
cd /opt/file-sync-tool-releases
nohup python3 serve.py 8080 > release-server.log 2>&1 &
```

常用检查命令：

```bash
tail -f /opt/file-sync-tool-releases/release-server.log
ps -ef | grep serve.py
```

注意：`nohup` 不会在服务器重启后自动重新启动服务。

## systemd 服务示例

长期部署建议使用 systemd。这样退出登录后服务仍会继续运行，也可以在服务器重启后自动启动。

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

保存服务配置后执行：

```bash
systemctl daemon-reload
systemctl enable --now file-sync-tool-releases
systemctl status file-sync-tool-releases
journalctl -u file-sync-tool-releases -f
```

## 发布 File Sync Tool 新版本

1. 在 Windows 构建机执行 `pnpm tauri:build:versioned-exe`。
2. 命令会生成带版本号和时间戳的 `.exe`，并创建或更新 `scripts/release-server/manifest.json`。
3. 在新增版本记录中填写 `changelog`。
4. 将生成的 `.exe` 和 `manifest.json` 复制到升级服务器根目录。

构建命令行为说明：

- `pnpm tauri:build:versioned-exe` 是本项目提供的自定义命令。
- `manifest.json` 按照 `version` 增量更新，不会重建整个历史记录。
- 重复构建同一版本时，会刷新 `url`、`sha256` 和 `released_at`，并保留已有的 `changelog`。
- 如果 `manifest.json` 格式错误，脚本会停止执行，不会覆盖原有发布历史。

替换程序升级包或 `manifest.json` 后，不需要重启升级服务器。

## WebView2 运行时文件

裸 `.exe` 启动时可以从升级服务器下载 WebView2 Runtime 安装程序。WebView2 文件独立于 `manifest.json`：

```text
<服务器根目录>/
|-- manifest.json                  # File Sync Tool 程序升级清单
|-- file-sync-tool-*.exe           # File Sync Tool 版本化程序文件
`-- webview2/
    |-- MicrosoftEdgeWebView2RuntimeInstallerX64.exe
    `-- MicrosoftEdgeWebView2RuntimeInstallerX64.exe.sha256
```

从微软下载“Evergreen Standalone Installer x64”：

https://developer.microsoft.com/microsoft-edge/webview2/

然后在 PowerShell 中生成对应的 SHA-256 文件：

```powershell
$f = "MicrosoftEdgeWebView2RuntimeInstallerX64.exe"
$hash = (Get-FileHash $f -Algorithm SHA256).Hash.ToLower()
"$hash  $f" | Out-File -Encoding ascii "$f.sha256" -NoNewline
```

## Notepad++ 插件目录

同一个静态服务器还可以托管 File Sync Tool“Notepad++ 扩展中心”使用的插件目录：

```text
<服务器根目录>/
`-- notepad-plugins/
    |-- catalog-v1.json
    `-- <插件名称>/<版本号>/<程序架构>/<插件安装包>.zip
```

例如：

```text
<服务器根目录>/
`-- notepad-plugins/
    |-- catalog-v1.json
    `-- EnhanceAnyLexer/
        `-- 1.4.1/
            |-- x86/EnhanceAnyLexer.zip
            |-- x64/EnhanceAnyLexer.zip
            `-- arm64/EnhanceAnyLexer.zip
```

如果 File Sync Tool 中配置的升级服务器地址为 `http://192.168.1.20:8080`，程序会读取：

```text
http://192.168.1.20:8080/notepad-plugins/catalog-v1.json
```

插件文件或 `catalog-v1.json` 替换后同样不需要重启服务器。目录字段、安装包要求和安全校验规则请参阅 [Notepad++ 插件目录说明](./notepad-plugins/README.md)。

## 视频设备模拟器素材目录

开发服务器也可托管测试、学习用途的视频设备模拟器素材；它只负责静态文件传输，不负责生成、签名或修改素材。生产静态服务器应发布由 `scripts/device-simulator-assets/asset-release.mjs` 完整校验的不可变 ZIP、Catalog 和分离签名，并按“ZIP 与签名先就绪，Catalog 最后原子替换”的顺序上线。

```text
<服务器根目录>/
`-- virtual-device-assets/
    |-- catalog-v1.json
    |-- catalog-v1.json.sig
    `-- packs/<pack-id>/<version>/<pack-id>-<version>.zip
```

素材已获准用于测试、学习、复制和打包，禁止商业使用；目录结构、缓存头、密钥边界和发布步骤见 [视频设备模拟器素材目录说明](./virtual-device-assets/README.md)。仓库和服务器目录均不得存放私钥。

当前版本的具体文件清单、Profile 最小依赖、客户端实际请求 URL 和上线验收步骤见 [视频设备模拟器素材部署指南](./virtual-device-assets/DEPLOYMENT_GUIDE.md)。
