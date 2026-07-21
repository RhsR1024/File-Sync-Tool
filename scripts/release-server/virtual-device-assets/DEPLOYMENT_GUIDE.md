# 视频设备模拟器素材部署指南

本文说明当前版本的 File Sync Tool 要使用“视频设备模拟器”时，升级/静态文件服务器上必须放置哪些文件，以及客户端如何访问这些文件。

## 1. 先说结论

当前 `catalog-v1.json`（生成时间 `2026-07-19T12:00:00+08:00`）声明了 6 个可选设备 Profile 和 8 个不可变 ZIP。要让当前客户端完整显示并使用全部 Profile，服务器上需要放置：

```text
<服务器根目录>/
├── manifest.json                         # 普通应用升级功能使用，可选于本功能
├── file-sync-tool-*.exe                  # 普通应用升级功能使用，可选于本功能
└── virtual-device-assets/
    ├── catalog-v1.json                   # 必需
    ├── catalog-v1.json.sig               # 必需
    └── packs/                             # 必需
        ├── ipc-custom/1.0.3/ipc-custom-1.0.3.zip
        ├── ipc-face-access/1.0.3/ipc-face-access-1.0.3.zip
        ├── ipc-smart/1.0.3/ipc-smart-1.0.3.zip
        ├── ipc-structured/1.0.3/ipc-structured-1.0.3.zip
        ├── media-h264-live/1.0.3/media-h264-live-1.0.3.zip
        ├── nvr-common/1.0.3/nvr-common-1.0.3.zip
        ├── nvr-vehicle/1.0.3/nvr-vehicle-1.0.3.zip
        └── protocol-core/1.0.3/protocol-core-1.0.3.zip
```

本目录中的 `README.md`、`NON_COMMERCIAL_NOTICE.txt`、`.gitignore` 和 `staging/` 不是客户端运行时下载的文件；`staging/` 不应发布为正式目录。

## 2. 当前 ZIP 清单

下表来自当前 `catalog-v1.json`，`size` 必须与服务器上实际文件的字节数一致，`sha256` 必须与文件内容一致。

| 用途 | 文件 | 大小（字节） | SHA-256 |
|---|---|---:|---|
| IPC Profile | `packs/ipc-custom/1.0.3/ipc-custom-1.0.3.zip` | 5,958,751 | `69c76a8fe88a64248cacd1065efc0f0d4413c3afd475d2631b3d579ae19b7b27` |
| IPC Profile | `packs/ipc-face-access/1.0.3/ipc-face-access-1.0.3.zip` | 4,583,989 | `1f70949ad114f0425e3ca40a1417fb919697d64dc4e1417681e3611b04466400` |
| IPC Profile | `packs/ipc-smart/1.0.3/ipc-smart-1.0.3.zip` | 29,086,926 | `9a48482d1e0901cc6093bc959412bf982655ca4afa9a07118bff9b4c88ca5193` |
| IPC Profile | `packs/ipc-structured/1.0.3/ipc-structured-1.0.3.zip` | 22,310,779 | `00c19bacecb7e95fe4deecd458bf954fbab4c76e9537b1fc96411c0dd7d61f5a` |
| 公共视频媒体 | `packs/media-h264-live/1.0.3/media-h264-live-1.0.3.zip` | 8,416,067 | `47f5e8001602d82d1aa82cd3bea1a786b91222ead85591d27acd601bae57df47` |
| NVR Profile | `packs/nvr-common/1.0.3/nvr-common-1.0.3.zip` | 18,519 | `b67cb7364e711f2595ee41aa125a95a438c4ee25655942f0fdcc55e9d95e6a77` |
| NVR Profile | `packs/nvr-vehicle/1.0.3/nvr-vehicle-1.0.3.zip` | 11,531,617 | `201358c8ad3bb888e78da9f9f560a701ef68893a3d80eadfc76974939e7c0673` |
| 公共协议能力 | `packs/protocol-core/1.0.3/protocol-core-1.0.3.zip` | 1,988,169 | `4fde02b3add428a4be4500816be85a19f60ecfd3fc59efb7fb7c0fe19f1d4b32` |

全量 ZIP 的 Catalog 声明大小合计为 `83,894,817` 字节，约 `80.0 MiB`。首次准备素材时，客户端还会写入本地缓存和解压目录，因此客户端磁盘需要预留明显高于 80 MiB 的空间。

## 3. Profile 与最小依赖

客户端不会盲目下载全部 ZIP，而是按照 `profiles[].required_packs` 和 `packs[].dependencies` 解析依赖：

| 可选 Profile | 必须具备的 ZIP |
|---|---|
| `ipc-custom` | `ipc-custom` + `media-h264-live` + `protocol-core` |
| `ipc-face-access` | `ipc-face-access` + `media-h264-live` + `protocol-core` |
| `ipc-smart` | `ipc-smart` + `media-h264-live` + `protocol-core` |
| `ipc-structured` | `ipc-structured` + `media-h264-live` + `protocol-core` |
| `nvr-common` | `nvr-common` + `media-h264-live` + `protocol-core` |
| `nvr-vehicle` | `nvr-vehicle` + `media-h264-live` + `protocol-core` |

因此：

- 只启用一个 Profile 时，最小发布集是 3 个 ZIP：该 Profile 包、`media-h264-live`、`protocol-core`。
- 希望页面列出并支持当前所有 Profile 时，直接发布上节列出的 8 个 ZIP。
- 不要只上传设备 Profile ZIP；缺少公共媒体包或协议包时，素材准备会失败。

## 4. 客户端访问地址

客户端的素材根地址有两种来源：

1. **默认方式**：使用“升级服务器地址”拼接 `/virtual-device-assets`。
   - 升级服务器：`http://192.168.1.20:8080`
   - 素材根地址：`http://192.168.1.20:8080/virtual-device-assets/`
2. **覆盖方式**：在“视频设备模拟器 → 高级设置 → 素材服务器”填写完整的 HTTP(S) 地址，例如 `http://192.168.1.20:8080/virtual-device-assets`。

客户端实际请求的固定文件为：

```text
<素材根地址>/catalog-v1.json
<素材根地址>/catalog-v1.json.sig
<素材根地址>/packs/<pack-id>/<version>/<pack-id>-<version>.zip
```

素材地址必须是绝对 `http://` 或 `https://` 地址，不能带用户名、密码或其他 HTTP Basic 认证信息。

## 5. 使用仓库自带的开发静态服务器

如果使用 `scripts/release-server/serve.py`，工作目录必须是包含 `virtual-device-assets/` 的服务器根目录，而不是 `virtual-device-assets/` 子目录：

```powershell
cd D:\WorkSpace\File-Sync-Tool\scripts\release-server
python serve.py 8080
```

然后客户端默认会访问：

```text
http://<服务器IP>:8080/virtual-device-assets/catalog-v1.json
```

`serve.py` 适合开发验证和内网小规模测试；生产环境应使用 Nginx、IIS、Caddy 或其他静态文件服务，并保持相同 URL 路径。

## 6. 发布顺序与签名要求

`catalog-v1.json` 不是普通配置文件。客户端会同时下载 Catalog 和 `.sig`，使用应用内置的 Ed25519 公钥验证原始字节、Catalog 结构、应用最低版本和每个 ZIP 的 SHA-256/大小。

当前签名包的 `key_id` 是 `device-assets-static-review-2026`。不要手工编辑 Catalog、改变换行、重新格式化 JSON 或重新压缩 ZIP；这些都会导致签名或哈希校验失败。私钥不得放在仓库、服务器静态目录或客户端安装目录。

推荐的上线顺序：

1. 上传全部新版本 ZIP，并确认 URL、文件大小、SHA-256 和 HTTP Range 可用。
2. 上传对应的 `catalog-v1.json.sig`。
3. 最后以原子替换方式发布 `catalog-v1.json`。

缓存建议：Catalog 和签名不使用长期缓存；版本化 ZIP 使用 `Cache-Control: public, max-age=31536000, immutable`。

## 7. 上线验收

在客户端执行素材准备前，先从客户端所在机器浏览器或网络工具检查：

```text
http://<服务器IP>:8080/virtual-device-assets/catalog-v1.json
http://<服务器IP>:8080/virtual-device-assets/catalog-v1.json.sig
http://<服务器IP>:8080/virtual-device-assets/packs/protocol-core/1.0.3/protocol-core-1.0.3.zip
```

验收标准：

- 三个地址都返回 HTTP 200；Catalog 是 JSON，签名文件是 JSON，ZIP 返回二进制内容。
- `catalog-v1.json` 中声明的 8 个 ZIP 都能按原路径访问。
- 当前客户端版本至少为 `1.2.0`；低于 `min_app_version: 1.2.0` 的客户端会拒绝此 Catalog。
- 在模拟器页面点击素材准备后，Profile 列表可加载，下载进度能完成，且没有“Catalog 未发布”“签名无效”或“依赖包缺失”错误。

## 8. 与现有说明的关系

- `scripts/release-server/README.md`：说明升级服务器根目录及 `virtual-device-assets/` 的整体位置。
- `scripts/release-server/virtual-device-assets/README.md`：说明发布根目录、不可变 ZIP、缓存头和私钥边界。
- `scripts/device-simulator-assets/README.md`：说明如何生成 Pack、生成/签名/校验 Catalog，以及如何执行 `publish`。
- 本文：补充当前 Catalog 的具体文件清单、Profile 最小依赖、客户端实际 URL 和部署验收步骤。

