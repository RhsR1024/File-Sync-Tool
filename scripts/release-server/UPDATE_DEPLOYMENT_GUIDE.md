# File Sync Tool 更新部署实施手册

本手册面向“在局域网或内网中部署更新服务器，并让已安装旧版本的客户端自动升级”的实际操作场景。

目标效果：

- 任意一台可以访问更新服务器的 Windows 客户端，只要当前安装的版本已经包含本项目的更新能力，就可以检查更新、下载新版本、自动关闭旧版本、替换自身并拉起新版本。
- 更新服务器只需要提供静态文件：`manifest.json` 和版本化的 `.exe` 文件。
- 可以使用本目录下的 `serve.py` 作为最小可用更新服务器。

## 1. 先回答一个关键问题

### 1.1 服务器是否必须安装 Python 3？

分两种情况：

- 如果你使用本项目自带的 `scripts/release-server/serve.py`，那么服务器上需要安装 `python3`。
- 如果你改用 Nginx、Apache、IIS、Caddy、对象存储静态站点或任何其他静态文件服务器，那么不需要 Python 3。

也就是说，项目的更新机制本质上不依赖 Python 3；只是当前仓库提供的最小实现 `serve.py` 依赖 Python 3。

### 1.2 客户端旧版本是否都能自动升级？

不是所有历史旧版本都一定可以。

前提条件：

- 这个“旧版本”本身已经包含当前这套 updater 功能。
- 客户端能访问你配置的更新服务器地址。

如果某个更早的历史版本还没有内置 updater，那么它需要手动安装一次“首个带 updater 的版本”，之后才可以进入自动升级链路。

## 2. 更新机制概览

客户端更新流程如下：

1. 客户端读取配置中的 `update_server_url`
2. 请求 `${update_server_url}/manifest.json`
3. 解析 `latest` 和 `versions[]`
4. 发现有更高版本后，下载对应 `.exe`
5. 下载完成后按 `sha256` 做完整性校验
6. 校验通过后把待安装信息写入本地配置
7. 用户点击“立即重启升级”
8. 当前旧版本自动关闭
9. helper bat 脚本等待旧进程退出后，用新 exe 替换旧 exe
10. 自动启动新版本

因此，服务器侧只需要稳定提供两个东西：

- `manifest.json`
- 新版本 `.exe`

## 3. 最终验收标准

部署完成后，应达到下面这个结果：

- 任意一台能连通更新服务器的客户端，安装了“带 updater 的旧版本”后，可以看到新版本提示。
- 客户端可以成功下载新版本。
- 客户端下载后会完成 SHA-256 校验。
- 用户点击应用更新后，旧版本自动退出。
- 新版本自动启动。
- 新版本启动后可以正常使用。

## 4. 服务器准备

## 4.1 基础要求

建议准备一台内网可访问的 Linux 服务器，也可以是 Windows 服务器。

最低要求：

- 客户端能通过 HTTP 或 HTTPS 访问这台服务器
- 服务器上有一个固定目录用于存放发布文件
- 如果使用 `serve.py`，需要安装 `python3`
- 放行对应端口，例如 `8080`

## 4.2 推荐目录结构

以 Linux 为例：

```text
/opt/file-sync-tool-releases/
  serve.py
  manifest.json
  file-sync-tool-1.0.8-202604261530.exe
  file-sync-tool-1.0.7-202604201020.exe
```

说明：

- `serve.py` 是静态文件服务脚本
- `manifest.json` 是更新索引文件
- 所有历史版本和当前最新版本的 `.exe` 都可以放在同一个目录

## 4.3 安装 Python 3

如果服务器没有 Python 3，可以按系统安装。

Ubuntu / Debian 示例：

```bash
sudo apt update
sudo apt install -y python3
python3 --version
```

CentOS / Rocky / AlmaLinux 示例：

```bash
sudo dnf install -y python3
python3 --version
```

如果你不想在服务器上安装 Python 3，请跳到“13. 可替代方案”，改用别的静态文件服务器。

## 5. 第一次部署更新服务器

## 5.1 创建发布目录

```bash
sudo mkdir -p /opt/file-sync-tool-releases
sudo chown -R $USER:$USER /opt/file-sync-tool-releases
cd /opt/file-sync-tool-releases
```

## 5.2 拷贝 `serve.py`

把仓库里的这个文件拷到服务器：

- `scripts/release-server/serve.py`

例如：

```bash
scp scripts/release-server/serve.py user@your-server:/opt/file-sync-tool-releases/
```

## 5.3 准备初始 `manifest.json`

建议第一次上线时，就把“当前已发给用户的版本”也写进 `manifest.json`，这样客户端历史版本列表不会是空的。

最小示例：

```json
{
  "latest": "1.0.7",
  "versions": [
    {
      "version": "1.0.7",
      "url": "file-sync-tool-1.0.7-202604201020.exe",
      "sha256": "在这里填写该exe的sha256",
      "released_at": "2026-04-20",
      "changelog": [
        "初始发布 updater 版本"
      ]
    }
  ]
}
```

规则：

- `latest` 必须是字符串版本号
- `versions` 必须是数组
- 新版本放前面，旧版本放后面
- 每个版本项必须包含：
  - `version`
  - `url`
  - `sha256`
  - `released_at`
  - `changelog`

## 5.4 上传初始 exe

把当前发布版本的 exe 拷到服务器发布目录，文件名建议直接使用构建脚本生成的版本化文件名。

## 5.5 启动服务

在发布目录中运行：

```bash
cd /opt/file-sync-tool-releases
python3 serve.py 8080
```

如果输出类似下面内容，说明服务已启动：

```text
[file-sync-tool-release] serving /opt/file-sync-tool-releases at http://0.0.0.0:8080
```

这时客户端实际访问的地址通常是：

```text
http://<服务器IP>:8080/manifest.json
```

## 5.6 用 systemd 设置为开机自启

在 Linux 上推荐这样做。

创建文件：

`/etc/systemd/system/file-sync-tool-releases.service`

内容如下：

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

然后执行：

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now file-sync-tool-releases
sudo systemctl status file-sync-tool-releases
```

注意：

- `WorkingDirectory` 必须指向发布目录
- `serve.py` 会把“当前工作目录”当成静态文件根目录

## 5.7 放行防火墙端口

例如放行 `8080`：

```bash
sudo ufw allow 8080/tcp
```

或：

```bash
sudo firewall-cmd --permanent --add-port=8080/tcp
sudo firewall-cmd --reload
```

## 6. 客户端配置

客户端需要把更新服务器地址配置为：

```text
http://<服务器IP>:8080
```

例如：

```text
http://192.168.1.10:8080
```

说明：

- 客户端会自动请求 `http://192.168.1.10:8080/manifest.json`
- 地址可以是 `http://` 或 `https://`
- 如果地址最后多写了 `/`，程序会自动去掉末尾斜杠
- 如果地址留空，则自动检查更新会被禁用

## 7. 发布一个新版本的完整步骤

以下步骤是每次发版都要做的标准流程。

## 7.1 在开发机上构建 release exe

在项目根目录执行：

```bash
pnpm tauri:build:versioned-exe
```

这个命令会：

- 执行 Tauri release 构建
- 自动把生成的 exe 重命名为带版本号和时间戳的文件名

示例输出文件名：

```text
file-sync-tool-1.0.8-202604261530.exe
```

重要说明：

- 只有 release 构建才能真正测试更新流程
- `pnpm tauri dev` 或 debug 构建里，更新检查和升级逻辑是禁用的

## 7.2 计算新 exe 的 SHA-256

在生成目录或服务器目录执行：

```bash
sha256sum file-sync-tool-1.0.8-202604261530.exe
```

示例输出：

```text
0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  file-sync-tool-1.0.8-202604261530.exe
```

取前面的 64 位十六进制字符串，写入 `manifest.json` 的 `sha256` 字段。

## 7.3 上传新 exe 到服务器

例如：

```bash
scp src-tauri/target/release/file-sync-tool-1.0.8-202604261530.exe user@your-server:/opt/file-sync-tool-releases/
```

## 7.4 更新 `manifest.json`

把新版本插到 `versions` 最前面，并把 `latest` 改成新版本。

示例：

```json
{
  "latest": "1.0.8",
  "versions": [
    {
      "version": "1.0.8",
      "url": "file-sync-tool-1.0.8-202604261530.exe",
      "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
      "released_at": "2026-04-26",
      "changelog": [
        "新增应用内更新流程",
        "优化错误码查询",
        "修复若干已知问题"
      ]
    },
    {
      "version": "1.0.7",
      "url": "file-sync-tool-1.0.7-202604201020.exe",
      "sha256": "旧版本sha256",
      "released_at": "2026-04-20",
      "changelog": [
        "上一版内容"
      ]
    }
  ]
}
```

注意：

- `url` 可以写相对路径
- 相对路径会自动相对于 `update_server_url` 解析
- `changelog` 必须是字符串数组
- 任何必填字段缺失，都可能导致该版本项被客户端丢弃

## 7.5 发布完成后不需要重启服务

无论是 `serve.py` 还是普通静态文件服务器，只要文件已经更新，请求下一次到来时就会读到最新内容。

## 8. 为什么客户端可以自动关闭旧版本并启动新版本

这是当前项目 updater 的既定行为。

升级应用时的动作如下：

1. 客户端先把新版 exe 下载到临时目录
2. 校验 SHA-256
3. 用户点击“立即重启升级”
4. 程序写出一个临时 bat 脚本
5. 当前旧版本关闭所有窗口并退出
6. bat 脚本等待旧进程真正结束
7. bat 脚本把下载好的新 exe 移动到当前程序路径，覆盖旧 exe
8. bat 脚本启动这个新 exe

所以正常情况下，用户会看到：

- 旧版本关闭
- 稍等片刻
- 新版本自动启动

只要：

- 下载成功
- `sha256` 正确
- 客户端本地对当前 exe 路径有写权限

更新后就应当能直接进入新版本并正常使用。

## 9. 首次上线 updater 的推荐实施顺序

如果目前线上还有“不带 updater”的老版本，建议按这个顺序推进：

1. 先发布一个“首个内置 updater 的版本”
2. 用人工方式让现有用户至少安装这一次
3. 在服务器上部署更新服务和 `manifest.json`
4. 从下一个版本开始走自动升级

原因：

- 不带 updater 的历史版本不会凭空获得自动升级能力
- 先把用户迁移到“带 updater 的基础版本”，后续才能自循环

## 10. 联调与验收步骤

建议至少用两台机器验证：

- A 机器：更新服务器
- B 机器：安装旧版本的客户端

验收步骤：

1. 在 A 机器部署好 `serve.py`、`manifest.json`、新旧两个版本的 exe
2. 在 B 机器安装一个“带 updater 的旧版本”
3. 在 B 机器配置更新服务器地址
4. 在浏览器中先访问 `http://<服务器IP>:8080/manifest.json`，确认可打开
5. 在客户端中点击“测试连接”，确认通过
6. 在客户端中点击“立即检查”
7. 确认客户端提示有新版本
8. 点击“立即升级”
9. 确认出现下载进度
10. 确认下载完成后进入“可应用更新”状态
11. 点击“立即重启升级”
12. 确认旧版本自动退出
13. 确认新版本自动拉起
14. 确认新版本号已经变成最新版本
15. 确认新版本可以正常进入主界面并执行基本功能

## 11. 建议做的回归检查

每次发版后，建议至少验证下面这些场景：

- 正常检查更新成功
- 正常下载更新成功
- 更新后自动重启成功
- `manifest.json` 可访问
- `sha256` 校验通过
- 旧版本升级后版本号正确
- 新版本启动后配置和基本功能正常

可选补充验证：

- 网络中断时下载失败提示是否合理
- 错误的 `sha256` 是否会阻止更新
- 更新地址为空时是否不会误报更新

## 12. 常见问题排查

## 12.1 浏览器打不开 `manifest.json`

优先检查：

- 服务器进程是否在运行
- 端口是否监听
- 防火墙是否放行
- 客户端和服务器网络是否互通
- `WorkingDirectory` 是否指向正确目录

## 12.2 客户端提示连接失败

检查：

- `update_server_url` 是否写错
- 地址是否能从客户端电脑直接访问
- 是不是把 `manifest.json` 放到了错误目录

## 12.3 客户端发现不了新版本

检查：

- `manifest.json` 的 `latest` 是否正确
- 新版本是否真的大于当前版本
- `versions[0]` 是否就是最新版本
- 当前客户端是不是 debug/dev 构建
- 当前客户端是否根本不带 updater

## 12.4 下载后校验失败

这通常说明：

- `manifest.json` 中的 `sha256` 写错了
- 上传到服务器的 exe 和你计算 hash 时的文件不是同一个
- 文件在上传过程中被替换了

修复方法：

1. 重新对服务器上的实际 exe 计算 `sha256`
2. 用真实值覆盖 `manifest.json`
3. 再次测试

## 12.5 点击应用更新后没有拉起新版本

检查：

- 客户端是否有权限覆盖当前 exe
- 杀毒软件是否阻止 bat 或 exe 替换
- 当前程序是否被其他进程占用
- 新 exe 是否真的存在于临时目录

## 12.6 为什么有的旧版本还是不能自动升级

原因通常是：

- 这个旧版本发布时还没有内置 updater

这种情况需要人工安装一次较新的“带 updater 的版本”，之后才可以持续自动升级。

## 13. 可替代方案

如果你不想在服务器上安装 Python 3，可以换成任意静态文件服务，只要满足下面条件即可：

- 能提供 `http(s)://<host>/<path>/manifest.json`
- 能提供 `manifest.json` 里引用的 `.exe` 文件
- 客户端可以直接访问

例如：

- Nginx
- Apache
- IIS
- Caddy
- NAS 自带 Web 服务

这时你仍然可以继续使用本手册里的：

- 目录结构
- `manifest.json` 格式
- 发版步骤
- 验收方法

只是把 `serve.py` 替换成你自己的静态文件服务方案。

## 14. 推荐的长期运维做法

- 保留最近几个历史版本 exe，便于排查和回退
- 每次发版后都先在测试机器验证一次自动升级
- 让 `manifest.json` 和实际服务器文件保持严格一致
- 不要手动重命名服务器上的 exe，除非同步修改 `manifest.json`
- 如果更换服务器地址，记得同步更新客户端配置

## 15. 最小上线清单

如果你想最快落地，只做下面这些也可以：

1. 服务器安装 `python3`
2. 创建 `/opt/file-sync-tool-releases`
3. 放入 `serve.py`
4. 放入 `manifest.json`
5. 放入至少一个版本化 `.exe`
6. 运行 `python3 serve.py 8080`
7. 客户端把更新地址配置成 `http://<服务器IP>:8080`
8. 用一台安装旧版本的客户端做完整升级验证

做到这一步，就已经具备基本的内网应用内升级能力。
