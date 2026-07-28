# 门户登录接口抓取指南

本工具通过 HTTP 接口自动完成门户认证登录，无需手动打开浏览器。首次使用前，需要确认你所在网络的登录接口信息是否已正确配置。

当前脚本已针对 **1.1.1.3 锐捷/深澜 AC 门户** 预配置好接口参数，大多数情况下只需修改 `config.ini` 中的账号密码即可使用。

如果你的环境不同，请按以下步骤抓取接口信息。

---

## 方法：使用浏览器开发者工具抓取

### 步骤 1：打开开发者工具

1. 打开 Chrome 浏览器，按 `F12` 打开开发者工具
2. 切换到 **Network（网络）** 标签页
3. 勾选 **Preserve log（保留日志）**

### 步骤 2：手动登录一次

1. 在浏览器中访问门户登录页面（如 `http://1.1.1.3`）
2. 正常输入用户名和密码，点击登录
3. 在 Network 面板中找到登录请求

### 步骤 3：识别登录请求

登录请求通常具有以下特征：
- **Method**: POST
- **URL** 可能包含 `login`、`auth`、`portal` 等关键字
- **Type**: `xhr` 或 `document`

对于锐捷/深澜 AC 门户：
- 请求 URL: `http://1.1.1.3/ac_portal/login.php`
- 请求方法: POST
- Content-Type: `application/x-www-form-urlencoded`

### 步骤 4：查看请求参数

点击该请求，查看 **Payload（负载）** 或 **Form Data**，你应该能看到类似：

```
opr=pwdLogin
userName=你的用户名
pwd=加密后的密码
auth_tag=时间戳密钥
rememberPwd=1
```

### 步骤 5：确认配置

将抓取到的信息与 `config.ini` 对比：

```ini
[portal]
HOST=http://1.1.1.3
LOGIN_URL=/ac_portal/login.php    ; 登录接口路径
PORTAL_URL=...                      ; 登录页面URL（用于获取Cookie）
```

如果你所在环境的接口路径不同，修改 `LOGIN_URL` 即可。

---

## 常见门户系统接口参考

| 门户系统 | 登录接口 | 说明 |
|---------|---------|------|
| 锐捷 AC Portal | `/ac_portal/login.php` | RC4加密，参数: opr=pwdLogin |
| 深澜 Portal | `/ac_portal/login.php` | 与锐捷类似 |
| H3C iMC | `/imc/j_spring_security_check` | 表单提交 |
| Cisco WLC | `/login.html` | 表单提交 |
| Ruijie SAM | `/ac_portal/login.php` | RC4加密 |

---

## 密码加密说明

本脚本实现了与网页端相同的 **RC4 加密**，流程如下：

1. 生成密钥：当前时间戳（毫秒）
2. 使用 RC4 算法加密明文密码，输出十六进制字符串
3. 将加密后的密码和密钥（`auth_tag`）一起提交

这样服务端可以用 `auth_tag` 解密出原始密码，实现了传输过程中的密码保护。

---

## 安全提醒

- `config.ini` 中的密码为**明文存储**，请注意保管
- 建议将脚本文件夹设置为仅当前用户可访问
- 不要将含真实密码的配置文件上传到公共仓库
- 分享给他人时，请删除 `config.ini` 中的密码，让对方自行填写
