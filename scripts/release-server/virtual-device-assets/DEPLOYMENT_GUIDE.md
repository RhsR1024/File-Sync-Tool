# 虚拟设备素材服务器部署（openEuler / Linux）

## 服务器目录

以当前服务器为例，发布根目录是 `/home/l10781/release_server`：

```text
/home/l10781/release_server/
├─ serve.py
├─ prepare-device-simulator-materials.sh
├─ prepare-device-simulator-materials.py
└─ virtual-device-assets/
   ├─ source-videos/                 # 发布端 MP4 输入，不向客户端分发
   ├─ prepared-videos/               # 脚本生成，客户端直接下载
   │  ├─ prepared-catalog.json       # 自动生成，无签名和素材版本号
   │  └─ media/themes/...            # 三码流 H.264 与 media.json 帧索引
   └─ alarm-images/
      ├─ person/person-001/{scene,person}.png
      ├─ face/face-001/{scene,face}.png
      ├─ car/car-001/{scene,vehicle,plate}.png
      └─ nonmotor/nonmotor-001/{scene,nonmotor}.png
```

`prepared-videos` 是客户端播放所必需的目录；`source-videos` 只是预处理输入；旧的空 `videos` 目录不再使用。

## 首次准备

服务器需要 Python 3 和带 `libx264` 编码器的 FFmpeg。安装方式取决于服务器已配置的软件源，安装后先检查：

```bash
python3 --version
ffmpeg -version
ffmpeg -hide_banner -encoders | grep libx264
```

将仓库中的两个脚本复制到 `/home/l10781/release_server/`，然后执行：

```bash
cd /home/l10781/release_server
chmod +x prepare-device-simulator-materials.sh prepare-device-simulator-materials.py
./prepare-device-simulator-materials.sh
```

脚本默认按自身所在目录定位素材，因此不需要填写绝对路径：

- 扫描 `./virtual-device-assets/source-videos/*.mp4`；
- 生成或更新 `./virtual-device-assets/prepared-videos`；
- 默认主题是 `车流测试视频.mp4`；
- 按源 MP4 的 SHA-256 内容身份复用未变化视频的已有结果。

如需修改默认主题：

```bash
./prepare-device-simulator-materials.sh --default-video '城市夜景-1.mp4'
```

如果 FFmpeg 不在 `PATH`：

```bash
./prepare-device-simulator-materials.sh --ffmpeg /opt/ffmpeg/bin/ffmpeg
```

## 后续更换素材

1. 在 `virtual-device-assets/source-videos` 中增删或替换 MP4，文件名就是客户端显示的主题名。
2. 保留现有 `prepared-videos`，再次运行 `./prepare-device-simulator-materials.sh`。
3. 客户端在虚拟设备页面点击“从服务器同步”。

不要在每次更新时清空 `prepared-videos`，否则所有 MP4 都会重新转码。脚本会自动删除已不在 `source-videos` 中的旧主题，并只重新处理内容发生变化的视频。临时转码目录使用隐藏名称，`serve.py` 不会将其列入下载清单；全部视频成功后才原子更新 catalog，因此日常增量生成期间可以保持服务运行。

`serve.py` 会动态发现生成结果。脚本和 `serve.py` 首次替换后需要按现有服务管理方式重启服务；以后仅更换素材并完成预处理时通常不需要重启。

## 告警图片

四类告警素材相互独立，每一组必须使用“类别 + 三位以上序号”的目录，并提供完整角色文件：

- `person/person-001`：`scene` 行人场景大图、`person` 同一行人的全身裁剪。
- `face/face-001`：`scene` 人脸场景/头肩上下文、`face` 同一人的脸部裁剪。
- `car/car-001`：`scene` 道路场景、`vehicle` 同一辆车的车身裁剪、`plate` 同一辆车的车牌裁剪。
- `nonmotor/nonmotor-001`：`scene` 非机动车场景、`nonmotor` 同一辆车的裁剪。

文件扩展名可为 `.jpg`、`.jpeg` 或 `.png`。新增第二组时直接加入 `person-002`、`face-002`、`car-002`、`nonmotor-002`；客户端会按组号排序并在连续告警事件间轮换，组内图片不会交叉拼接。客户端同步后生成大、中、小 JPEG 缓存，这一步不依赖 FFmpeg。

客户端也可点击“清理并重新同步”。该操作只删除升级服务器同步清单中登记的素材，不删除手工导入的告警图片或未登记的自定义文件。

客户端只下载 `alarm-images` 和 `prepared-videos`，不会下载源 MP4，不运行 FFmpeg，也不会再次转码。`files.json` 由 `serve.py` 动态生成；SHA-256 只用于精确判断缓存内容是否相同，不是签名或防篡改门槛。

## Windows 本地预生成（可选）

`prepare-device-simulator-materials.ps1` + Windows 裸 EXE 仅供 Windows 发布机使用。openEuler 服务器应使用上面的 `.sh` + `.py`，不能运行 Windows 裸 EXE。
