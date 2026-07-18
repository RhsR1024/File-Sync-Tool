# Virtual device asset release root

该目录是视频设备模拟器素材在升级/静态服务器上的发布根目录模板。实际 ZIP、Catalog 和签名属于发布产物，不提交到 Git。

生产目录结构：

```text
virtual-device-assets/
├── catalog-v1.json
├── catalog-v1.json.sig
└── packs/
    └── <pack-id>/
        └── <version>/
            └── <pack-id>-<version>.zip
```

发布规则：

- 素材仅限已确认授权的非商业测试、学习、复制和打包用途；禁止商业使用。
- `pack-id + version` 一经发布不得替换，变更内容必须提升版本。
- 先上传并校验所有 ZIP，最后使用 `scripts/device-simulator-assets/asset-release.mjs publish` 原子更新 Catalog。
- `catalog-v1.json` 使用 `Cache-Control: no-cache`；版本化 ZIP 使用长期 `Cache-Control: public, max-age=31536000, immutable`。
- 签名私钥永远不得放入本目录。这里只发布 Catalog、分离签名、不可变 ZIP；公钥由应用的受信密钥配置分发。
- 仓库中的 `scripts/release-server/serve.py` 仅适合开发验证，不作为大规模生产素材分发服务。

完整生成、签名、校验、发布和密钥轮换步骤见 [`scripts/device-simulator-assets/README.md`](../../device-simulator-assets/README.md)。
