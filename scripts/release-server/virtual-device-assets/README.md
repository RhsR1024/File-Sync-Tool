# 虚拟设备松散素材分发目录

本目录由升级服务器直接发布。openEuler/Linux 服务器将 MP4 放入 `source-videos` 后，通过发布根目录下的 `prepare-device-simulator-materials.sh` 一次性生成 `prepared-videos`；客户端只下载可直接播放的结果，不需要 FFmpeg。无需签名、素材版本号、ZIP 包或人工维护 catalog；`serve.py` 会动态提供 `files.json`。

目录结构和操作方式见 [`DEPLOYMENT_GUIDE.md`](DEPLOYMENT_GUIDE.md)。
