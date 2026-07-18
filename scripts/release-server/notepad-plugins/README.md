# Notepad++ 插件目录说明

本目录用于维护 File Sync Tool“Notepad++ 扩展中心”的插件清单和插件安装包。File Sync Tool 会自动识别所选 Notepad++ 的程序架构，并只展示匹配的插件安装包。

## 访问地址

插件清单固定发布在：

```text
http://<升级服务器>/notepad-plugins/catalog-v1.json
```

例如升级服务器地址为 `http://192.168.1.20:8080` 时，插件清单地址为：

```text
http://192.168.1.20:8080/notepad-plugins/catalog-v1.json
```

## 推荐目录结构

```text
notepad-plugins/
|-- catalog-v1.json
`-- EnhanceAnyLexer/
    `-- 1.4.1/
        |-- x86/EnhanceAnyLexer.zip
        |-- x64/EnhanceAnyLexer.zip
        `-- arm64/EnhanceAnyLexer.zip
```

通用目录规则：

```text
notepad-plugins/<插件名称>/<版本号>/<程序架构>/<安装包>.zip
```

ZIP 文件名可以自定义，例如 `EnhanceAnyLexer_x64.zip`；只要 `catalog-v1.json` 中的 `url` 与服务器上的实际文件名完全一致即可。程序架构以所在目录和清单中的 `packages` 键为准，因此 x64 安装包必须放入 `x64` 目录，不能放入 `x86` 目录。

支持的架构标识为：

- `x86`：32 位 Notepad++。
- `x64`：64 位 Notepad++。
- `arm64`：ARM64 Notepad++。

上游插件没有提供某个架构时，不要创建空目录，也不要在 `catalog-v1.json` 中声明该架构。

## EnhanceAnyLexer 示例

将从上游下载的不同架构 ZIP 文件放到：

```text
notepad-plugins/EnhanceAnyLexer/1.4.1/x86/EnhanceAnyLexer.zip
notepad-plugins/EnhanceAnyLexer/1.4.1/x64/EnhanceAnyLexer.zip
notepad-plugins/EnhanceAnyLexer/1.4.1/arm64/EnhanceAnyLexer.zip
```

对应的清单版本记录示例：

```json
{
  "version": "1.4.1",
  "notepad_compatible": "[8.4.3,]",
  "packages": {
    "x64": {
      "url": "EnhanceAnyLexer/1.4.1/x64/EnhanceAnyLexer.zip",
      "sha256": "<64 位小写 SHA-256>",
      "size": 123456,
      "install_dir": "EnhanceAnyLexer",
      "entry_dll": "EnhanceAnyLexer.dll"
    }
  }
}
```

`url` 是相对于 `notepad-plugins/` 的路径。上面的安装包实际下载地址为：

```text
http://<升级服务器>/notepad-plugins/EnhanceAnyLexer/1.4.1/x64/EnhanceAnyLexer.zip
```

## catalog-v1.json 字段说明

插件级字段：

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `id` | 是 | 插件的稳定唯一标识，只使用英文字母、数字、短横线或下划线。 |
| `name` | 是 | 界面展示的插件名称。 |
| `publisher` | 是 | 插件作者或发布者。 |
| `description_zh` | 是 | 中文功能说明。 |
| `description_en` | 是 | 英文功能说明，用于 File Sync Tool 英文界面。 |
| `homepage` | 是 | 插件原始项目或官方网站。 |
| `license` | 是 | 插件许可证，例如 `MIT`。 |
| `adapter` | 否 | File Sync Tool 配置适配器标识；没有可视化配置时可以省略或留空。 |
| `releases` | 是 | 插件版本列表，建议按新版本到旧版本排列。 |

版本和安装包字段：

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `version` | 是 | 插件版本号。 |
| `notepad_compatible` | 是 | 适用的 Notepad++ 版本范围，仅用于展示和审核。 |
| `packages` | 是 | 按照 `x86`、`x64`、`arm64` 保存的安装包信息。 |
| `url` | 是 | 相对于 `notepad-plugins/` 的安装包路径，也支持完整的 HTTP/HTTPS 地址。 |
| `sha256` | 是 | 安装包 SHA-256，使用 64 位小写十六进制字符。 |
| `size` | 是 | 安装包字节数，用于目录展示和审核。 |
| `install_dir` | 是 | Notepad++ `plugins` 下的插件目录名称。 |
| `entry_dll` | 是 | ZIP 中必须存在的插件入口 DLL。 |

## 计算安装包信息

在 Linux 升级服务器上执行：

```bash
sha256sum EnhanceAnyLexer_x64.zip
stat -c %s EnhanceAnyLexer_x64.zip
```

将第一条命令输出开头的 64 位哈希写入 `sha256`，将第二条命令输出的字节数写入 `size`。

在 Windows PowerShell 中执行：

```powershell
$file = "EnhanceAnyLexer.zip"
(Get-FileHash $file -Algorithm SHA256).Hash.ToLower()
(Get-Item $file).Length
```

将第一行结果写入 `sha256`，将第二行字节数写入 `size`。

## 安装包要求与安全校验

- 建议保留上游发布的原始 ZIP，不要重新压缩或修改内容。
- ZIP 内必须包含清单中 `entry_dll` 指定的插件 DLL。
- 插件 DLL 架构必须与当前 Notepad++ 架构一致。
- File Sync Tool 会拒绝包含绝对路径、父目录穿越或符号链接的压缩包。
- File Sync Tool 会拒绝超过大小限制、SHA-256 不一致或缺少入口 DLL 的安装包。
- 更新已安装插件前，File Sync Tool 会把旧插件备份到自身应用数据目录。
- 如果 Notepad++ 正在占用插件 DLL，应先退出 Notepad++ 再更新插件。
- 安装到 `Program Files` 下的 Notepad++ 时，可能需要以管理员身份运行 File Sync Tool。

## 第三方插件许可证

将第三方插件放入内网升级服务器前，必须确认其许可证允许内部再分发。每个插件清单都应保留原始项目地址和许可证信息。

插件不允许再分发时，可以只登记配置适配器和使用说明，不要将其二进制安装包上传到服务器。

## 发布检查清单

1. 确认插件来源和许可证。
2. 分别下载插件实际支持的架构版本。
3. 将 ZIP 放入对应的插件、版本和架构目录。
4. 计算每个 ZIP 的 SHA-256 和字节数。
5. 更新 `catalog-v1.json` 中的 `releases`。
6. 在浏览器中确认清单和 ZIP 地址可以访问。
7. 使用对应架构的 Notepad++ 完成一次安装和启动验证。

替换安装包或 `catalog-v1.json` 后不需要重启静态文件服务器。
