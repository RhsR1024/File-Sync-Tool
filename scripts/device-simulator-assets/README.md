# 虚拟设备素材工具（历史目录）

当前素材发布不再使用签名 catalog、版本号或版本化 ZIP。本目录旧脚本仅保留用于历史测试与格式迁移。

日常分发时，把 MP4 放入升级服务器的 `virtual-device-assets/source-videos`，运行 `prepare-device-simulator-materials.ps1`；客户端随后点击“从服务器同步”，无需安装 FFmpeg。完整说明见 [`docs/device-simulator-local-materials.md`](../../docs/device-simulator-local-materials.md)。

素材生成完成后，可在构建机或 CI 上对任一路输出执行 VMAF 门禁。该命令只读取源 MP4 与 `media.json`，不会成为桌面客户端的运行时依赖：

```powershell
pnpm quality:device-simulator-assets -- --source <source.mp4> --manifest <media.json> --minimum-vmaf 80 --sample-seconds 5
```

门禁按 manifest 声明的帧率重建 H.264 时间线，并采用与素材生成相同的 Lanczos 缩放方式比较画质。需要支持 `libvmaf` 的 FFmpeg；可用 `--ffmpeg` 和 `--ffprobe` 指定工具路径。
