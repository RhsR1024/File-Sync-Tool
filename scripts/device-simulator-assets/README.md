# 视频设备模拟器素材发布工具

本目录提供 Pack/Catalog 的确定性生成、Ed25519 离线签名、完整校验和 Catalog-last 发布工具。它只处理数据素材，不把素材或签名私钥编入应用 EXE。

## 授权边界

当前素材边界固定为非商业用途：

> Authorized for testing, learning, copying, and packaging; commercial use is prohibited.

每份 Pack 源定义和生成的 `pack.json` 都必须原样包含这段声明。工具会拒绝缺失或改写声明的输入。只有已确认具备内部复制、派生和发布授权的模板、图片、PCAP 转换结果及其他素材才可进入源目录；本工具不会替代授权审查，也不表示已经过真实平台兼容性验证。

## 前置条件

- 使用项目支持的 Node.js 版本，无额外 npm 依赖。
- 私钥必须是 Ed25519 PKCS#8 PEM，放在仓库和发布目录之外。
- 公钥必须是 Ed25519 SPKI PEM；公钥可以交付给应用或发布系统。
- 发布目录所在文件系统必须支持同目录原子重命名。生产发布建议在 Linux/Nginx、IIS 或等价静态服务的部署侧执行。

可用 OpenSSL 在受控目录创建密钥：

```powershell
openssl genpkey -algorithm Ed25519 -out D:\SecureKeys\device-assets-2026.key
openssl pkey -in D:\SecureKeys\device-assets-2026.key -pubout -out D:\SecureKeys\device-assets-2026.pub.pem
```

不要把私钥复制到仓库、构建产物、发布服务器静态目录、日志、工单或聊天记录。CLI 会拒绝仓库内的私钥路径，并且只输出可公开配置的原始公钥 Base64。

## 发布流程

1. 复制并修改 [`pack-source.example.json`](./pack-source.example.json)，让 `source_dir` 指向已授权的纯数据素材目录。
2. 为每个新版本生成不可变 Pack：

   ```powershell
   node scripts/device-simulator-assets/asset-release.mjs pack --definition D:\ReleaseWork\ipc-custom.pack.json --release-root D:\ReleaseWork\virtual-device-assets
   ```

3. 复制并修改 [`catalog-source.example.json`](./catalog-source.example.json)。`generated_at` 必须显式填写 RFC 3339 时间；`min_app_version` 必须使用当次批准的真实应用版本。
4. 从已经存在的 Pack 生成未签名暂存 Catalog。哈希和大小由工具读取 ZIP 得出，不能手填：

   ```powershell
   node scripts/device-simulator-assets/asset-release.mjs catalog --definition D:\ReleaseWork\catalog-source.json --release-root D:\ReleaseWork\virtual-device-assets
   ```

5. 使用仓库外的私钥对 Catalog 的原始字节签名：

   ```powershell
   node scripts/device-simulator-assets/asset-release.mjs sign --catalog D:\ReleaseWork\virtual-device-assets\staging\catalog-v1.json --private-key D:\SecureKeys\device-assets-2026.key --key-id device-assets-2026
   ```

6. 上传所有新版本 ZIP，重新计算服务器侧 SHA-256，确认 URL、`Content-Length` 和 Range 请求可用。版本 ZIP 应返回长期 `immutable` 缓存；Catalog 应返回 `no-cache`。
7. 在发布目录执行全量校验：

   ```powershell
   node scripts/device-simulator-assets/asset-release.mjs validate --release-root D:\ReleaseWork\virtual-device-assets --catalog D:\ReleaseWork\virtual-device-assets\staging\catalog-v1.json --signature D:\ReleaseWork\virtual-device-assets\staging\catalog-v1.json.sig --public-key D:\SecureKeys\device-assets-2026.pub.pem --key-id device-assets-2026
   ```

8. 最后发布 Catalog。工具会先校验候选 Catalog、签名和全部 Pack，然后先安装签名，最后以 `catalog-v1.json` 的原子替换作为提交点：

   ```powershell
   node scripts/device-simulator-assets/asset-release.mjs publish --release-root D:\ReleaseWork\virtual-device-assets --catalog D:\ReleaseWork\virtual-device-assets\staging\catalog-v1.json --signature D:\ReleaseWork\virtual-device-assets\staging\catalog-v1.json.sig --public-key D:\SecureKeys\device-assets-2026.pub.pem --key-id device-assets-2026
   ```

不得先发布 Catalog 再上传 ZIP，也不要在签名后格式化、换行转换或以其他方式重写 Catalog。签名覆盖 Catalog 的精确原始字节。

## 不可变性和失败处理

- 目标路径固定为 `packs/<id>/<version>/<id>-<version>.zip`。
- 相同 `id + version` 再次生成相同字节时为幂等成功；字节不同会报 `immutable_pack_conflict`，必须提升版本，工具不会覆盖旧包。
- ZIP 使用稳定排序、固定时间戳和 STORE 格式，因此同一输入产生同一字节。
- 工具拒绝符号链接、路径穿越、Windows 不安全路径、大小/数量超限、未声明文件及 EXE/DLL/PY/JS/BAT/CMD/PS1/WASM 等可执行内容。
- 发布失败时不要手工拼接半成品 Catalog。修复问题后重新运行 `validate` 和 `publish`。

## 密钥轮换

1. 在受控的仓库外目录生成新密钥，并分配新的稳定 `key_id`。
2. 先发布包含新公钥的应用配置或受信密钥集合，并保留旧公钥覆盖升级窗口。
3. 确认客户端已信任新公钥后，才使用新私钥签署 Catalog。
4. 升级窗口结束后再从后续应用版本移除旧公钥。历史 Catalog 与 Pack 保持不可变。

私钥泄露时应立即停止发布、轮换 `key_id` 和密钥，并按事件响应流程处置；不能只重新生成同名私钥。

## 本地测试

```powershell
node --test scripts/device-simulator-assets/asset-release.test.mjs
```

测试使用系统临时目录动态生成短期密钥，不创建或提交实际发布密钥。
