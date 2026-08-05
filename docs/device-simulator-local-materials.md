# 虚拟设备素材发布与客户端缓存

虚拟设备视频采用“Linux 升级服务器一次预处理、Windows 客户端直接播放”的模式。普通客户端不需要安装 FFmpeg，不使用素材签名、素材版本号、加密 catalog 或版本化 ZIP。

## openEuler 发布端流程

将以下脚本放在升级服务器发布根目录（例如 `/home/l10781/release_server`）：

- `prepare-device-simulator-materials.sh`
- `prepare-device-simulator-materials.py`

将 MP4 放入同级 `virtual-device-assets/source-videos`，然后运行：

```bash
cd /home/l10781/release_server
chmod +x prepare-device-simulator-materials.sh prepare-device-simulator-materials.py
./prepare-device-simulator-materials.sh
```

发布服务器需要 Python 3，以及带 `libx264` 编码器的 FFmpeg。脚本生成：

- 1920×1080、25 FPS 主码流；
- 640×360、20 FPS 子码流和第三码流；
- 可按帧读取的二进制 H.264 与 JSON 索引；
- 自动主题映射和默认主题。

生成结果位于 `virtual-device-assets/prepared-videos`。`serve.py` 自动扫描这些文件并提供轻量 `files.json`，不需要人工编辑 catalog。MP4 内容没有变化时，脚本按 SHA-256 精确复用原结果，重命名文件也不会重新转码。

当前六项实况全部由升级服务器分发：“凡人修仙传”“云海－剪影－励志”“交通监控测试视频”“城市夜景-1”“城市夜景-2”“车流测试视频”。服务器同步完成后默认选中并保存“车流测试视频”。告警图片同样由服务器下载，EXE 不再内置 JPEG 或 H.264 素材。

告警图片按独立类别、组号和角色文件发布，例如 `car/car-001/{scene,vehicle,plate}.png`。行人、人脸、机动车、非机动车分别需要 2、2、3、2 张图片；新增 `*-002` 等完整目录后，连续告警会逐组轮换，同一事件始终使用同一组素材。

Windows 发布机仍可使用 `prepare-device-simulator-materials.ps1` + 裸 EXE 预生成同格式结果，但该流程不能用于 openEuler。

## 客户端流程

客户端在虚拟设备页面点击“从服务器同步”：

1. 流式下载变化的已处理媒体和告警图片；
2. 用完整 SHA-256 内容身份跳过未变化文件；
3. 校验媒体结构确实可读取；
4. 直接显示新的实况主题。

客户端不会下载源 MP4，不运行 FFmpeg，也不做视频转码。虚拟设备首次使用前必须至少成功执行一次“从服务器同步”；服务器删除的主题会在同步成功后从客户端服务器素材缓存中移除。

“清理并重新同步”只清理升级服务器同步状态中登记的文件，随后重新下载并重建索引；手工导入的告警图片和未登记的普通自定义文件不在删除范围内。

客户端仍保留“本地刷新”能力，供开发人员临时直接向本机 `videos` 放 MP4；只有这种本地开发用法才需要本机 FFmpeg。

## 100 路设备时的资源模型

- H.264 文件不会整体载入内存，只按当前帧索引读取。
- 同一主题的主、子、第三码流各使用一条共享时间线，虚拟相机共享媒体对象。
- 没有 RTSP 订阅时不读取视频帧。
- 时间水印最多使用三条全局共享处理管线，不按相机数量创建编码器。
- 100 路相机主要增加监听、连接状态和 RTP 封装开销，不会复制 100 份完整视频。
