# 代码修改统计系统

[![Go Version](https://img.shields.io/badge/Go-1.21+-blue.svg)](https://golang.org/)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows-lightgrey.svg)](https://www.microsoft.com/windows)

基于Go语言开发的高性能代码修改统计系统，用于对比两个代码目录并分析代码和注释的变更情况。支持实时进度显示、可视化图表展示和多格式导出。

## 🌟 功能特性

### 核心功能
- 📊 **智能代码分析**：支持多种编程语言的代码和注释识别
- 🔄 **实时进度显示**：WebSocket式的实时任务进度更新
- 📈 **丰富的可视化图表**：柱状图、饼图、条形图等多种图表类型
- 📋 **详细统计报告**：文件级别、总体统计、操作维度、文件类型维度
- 💾 **多格式导出**：支持JSON、CSV格式导出
- 🌐 **Web界面**：现代化的响应式Web界面
- 📦 **单一可执行文件**：前端资源完全嵌入，无外部依赖

### 优化特性
- ⚡ **只显示变更文件**：过滤无变更文件，提高输出精准性
- 🎨 **离线运行**：无需外网访问，适用于内网环境
- 📊 **详细统计面板**：代码/注释分类统计，包含净变更计算
- 🎯 **高性能处理**：支持大型代码库的快速分析

## 📁 支持的文件类型

| 语言/类型 | 扩展名 | 单行注释 | 多行注释 | 用途说明 |
|-----------|--------|----------|----------|----------|
| Go | `.go` | `//` | `/* */` | Go语言源码 |
| Java | `.java` | `//` | `/* */` | Java语言源码 |
| C/C++ | `.c`, `.cpp`, `.cc` | `//` | `/* */` | C/C++语言源码 |
| JavaScript/TypeScript | `.js`, `.ts` | `//` | `/* */` | 前端脚本文件 |
| Python | `.py` | `#` | - | Python脚本 |
| Shell | `.sh` | `#` | - | Shell脚本 |
| **SQL** | `.sql` | `--` | `/* */` | **数据库查询文件** |
| **XML** | `.xml` | - | `<!-- -->` | **MyBatis映射文件等** |
| HTML | `.html` | - | `<!-- -->` | 网页标记文件 |
| CSS | `.css` | - | `/* */` | 样式表文件 |
| SCSS/LESS | `.scss`, `.less` | `//` | `/* */` | 预处理器样式 |
| PHP | `.php` | `//`, `#` | `/* */` | PHP服务端脚本 |
| Ruby | `.rb` | `#` | `=begin =end` | Ruby脚本 |
| Perl | `.pl` | `#` | - | Perl脚本 |
| Lua | `.lua` | `--` | `--[[ ]]` | Lua脚本 |
| R | `.r` | `#` | - | R统计脚本 |
| Swift | `.swift` | `//` | `/* */` | iOS开发语言 |
| Kotlin | `.kt` | `//` | `/* */` | Android开发语言 |
| Dart | `.dart` | `//` | `/* */` | Flutter开发语言 |
| Rust | `.rs` | `//` | `/* */` | Rust系统编程语言 |
| Scala | `.scala` | `//` | `/* */` | Scala函数式语言 |

## 🚀 快速开始

### 前置要求

- Go 1.21+
- Windows操作系统
- 网络连接（仅用于下载Go依赖，运行时无需网络）

### 安装和编译

1. **克隆项目**
   ```bash
   git clone <repository-url>
   cd code-count
   ```

2. **初始化依赖**
   ```bash
   go mod tidy
   ```

3. **编译项目**
   ```bash
   go build -o code-count.exe main.go
   ```

   或者使用提供的构建脚本：
   ```bash
   build.bat
   ```

### 运行程序

```bash
# 直接运行编译后的可执行文件
code-count.exe

# 或者直接运行源码
go run main.go
```

程序启动后，访问 http://localhost:8084

## 📊 界面展示

### 主要功能区域

1. **配置区域**
   - 旧版本路径输入
   - 新版本路径输入
   - 开始分析按钮

2. **进度显示**
   - 实时进度条
   - 当前处理文件信息
   - 阶段提示（扫描/对比/统计）

3. **结果展示**
   - 汇总统计卡片（新增/删除/修改总数）
   - 详细统计面板（代码/注释分类）
   - 多种可视化图表
   - 变更文件类型统计
   - 文件详细列表

4. **导出功能**
   - JSON格式导出
   - CSV格式导出

### 图表类型

| 图表类型 | 描述 | 展示内容 |
|----------|------|----------|
| 📊 柱状图 | 代码与注释变更对比 | 新增/删除/修改的具体数值 |
| 🥧 饼图 | 变更类型分布 | 新增/删除/修改的比例关系 |
| 📈 条形图 | 变更文件类型统计 | 各文件类型的变更量排序 |

## 🔧 API 接口

### RESTful API

#### 创建分析任务
```http
POST /api/tasks
Content-Type: application/json

{
  "oldPath": "C:/old-code",
  "newPath": "C:/new-code"
}
```

**响应**：
```json
{
  "taskId": "uuid-string"
}
```

#### 查询任务进度
```http
GET /api/progress/{taskId}
```

**响应**：
```json
{
  "taskId": "uuid-string",
  "status": "running|completed|failed",
  "progress": {
    "phase": "scan|diff|stats|completed",
    "currentFile": "src/main.go",
    "processedFiles": 5,
    "totalFiles": 20,
    "percent": 25
  },
  "error": "error message if failed"
}
```

#### 获取分析结果
```http
GET /api/results/{taskId}
```

**响应**：
```json
{
  "files": [
    {
      "filePath": "src/main.go",
      "codeAdded": 10,
      "codeDeleted": 5,
      "codeModified": 3,
      "commentAdded": 2,
      "commentDeleted": 1,
      "commentModified": 0
    }
  ],
  "summary": {
    "codeAdded": 100,
    "codeDeleted": 80,
    "codeModified": 30,
    "commentAdded": 20,
    "commentDeleted": 10,
    "commentModified": 5
  },
  "operationSummary": {
    "addedTotal": 120,
    "deletedTotal": 90,
    "modifiedTotal": 35
  },
  "fileTypeSummary": {
    ".go": {
      "codeAdded": 70,
      "codeDeleted": 50,
      "codeModified": 20,
      "commentAdded": 10,
      "commentDeleted": 5,
      "commentModified": 2
    }
  }
}
```

#### 导出结果
```http
GET /api/results/{taskId}/export?format=json
GET /api/results/{taskId}/export?format=csv
```

## 📂 项目结构

```
code-count/
├── main.go                 # 主程序入口
├── go.mod                  # Go模块依赖
├── go.sum                  # 依赖校验文件
├── build.bat              # Windows构建脚本
├── README.md              # 项目文档
├── internal/              # 内部包
│   ├── models/            # 数据模型
│   │   └── models.go
│   ├── services/          # 业务逻辑
│   │   ├── scanner.go     # 文件扫描和对比
│   │   └── task_manager.go # 任务管理
│   ├── api/               # REST API接口
│   │   ├── handler.go     # 请求处理器
│   │   └── router.go      # 路由配置
│   └── utils/             # 工具函数
│       └── comment.go     # 注释识别
└── web/                   # 前端资源
    └── dist/
        └── index.html     # 前端页面
```

## 🎯 使用场景

### 开发团队
- **版本对比**：对比不同版本间的代码变更
- **代码审查**：量化代码修改内容
- **工作量统计**：统计开发工作量
- **质量分析**：分析代码和注释的比例

### 项目管理
- **进度跟踪**：可视化项目进展
- **资源评估**：评估开发资源投入
- **风险识别**：识别大量变更的风险区域

### 运维团队
- **部署分析**：分析部署包的变更内容
- **回滚准备**：了解回滚影响范围

## 💡 使用技巧

### 路径设置
- 支持绝对路径和相对路径
- Windows路径可使用 `\` 或 `/` 分隔符
- 路径中包含空格时会自动处理

### 性能优化
- 建议对大型项目分模块进行分析
- 排除不必要的文件（如构建产物、依赖库）
- 使用SSD硬盘可显著提升处理速度

### 结果解读
- **净变更** = 新增行数 - 删除行数
- **只显示有变更的文件**，提高关注焦点
- **变更文件类型统计**帮助了解项目技术栈分布

## ⚠️ 注意事项

1. **路径验证**：程序启动前会验证路径是否存在
2. **文件权限**：确保对目标目录有读取权限
3. **大文件处理**：超大文件可能需要较长处理时间
4. **编码支持**：支持UTF-8编码的文本文件
5. **内存使用**：大型项目分析时会占用较多内存

## 🔍 故障排除

### 常见问题

**Q: 程序无法启动**
A: 检查Go版本是否为1.21+，确保所有依赖已正确下载

**Q: 分析速度很慢**
A: 检查路径是否包含大量无关文件，建议排除node_modules等目录

**Q: 某些文件未被识别**
A: 检查文件扩展名是否在支持列表中，可以扩展utils/comment.go添加新的文件类型

**Q: Web界面无法访问**
A: 确认端口8084未被占用，检查防火墙设置

### 日志输出
程序运行时会在控制台输出详细的处理信息：
- 启动信息
- 文件读取警告
- 错误信息

## 🛠️ 开发说明

### 添加新的文件类型
编辑 `internal/utils/comment.go` 文件，在 `GetCommentRules()` 函数中添加新的文件类型规则：

```go
".新扩展名": {
    SingleLine: []string{"//"},
    MultiStart: []string{"/*"},
    MultiEnd: []string{"*/"}
},
```

### 自定义端口
修改 `main.go` 文件中的端口设置：

```go
port := ":8084"  // 修改为你需要的端口
```

### 扩展API
在 `internal/api/handler.go` 中添加新的API端点

## 📄 许可证

本项目采用 MIT 许可证 - 查看 [LICENSE](LICENSE) 文件了解详情

## 🤝 贡献

欢迎提交Issue和Pull Request来改进这个项目！

## 📞 支持

如果你在使用过程中遇到问题，可以：
- 提交Issue到项目仓库
- 查看故障排除部分
- 检查项目文档

---

**作者**: l10840
**创建时间**: 2025-09-18
**版本**: v1.0.0
