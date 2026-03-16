# 使用示例

本文档提供了代码修改统计系统的详细使用示例。

## 基本使用流程

### 1. 启动程序
```bash
# 方式1: 运行编译后的可执行文件
code-count.exe

# 方式2: 直接运行源码
go run main.go
```

程序启动后，你会看到以下输出：
```
Starting Code Count Analysis Server...
Server starting on http://localhost:8083
```

### 2. 访问Web界面
在浏览器中打开：http://localhost:8083

### 3. 配置分析参数
- **旧版本路径**: 输入要对比的旧版本代码目录
- **新版本路径**: 输入要对比的新版本代码目录

示例路径：
```
旧版本路径: C:\project\v1.0
新版本路径: C:\project\v2.0
```

### 4. 开始分析
点击"开始分析"按钮，系统会：
- 验证路径有效性
- 扫描文件结构
- 执行差异对比
- 生成统计报告

### 5. 查看结果
分析完成后，界面会显示：
- 汇总统计卡片
- 详细统计面板
- 可视化图表
- 文件详细列表

## API使用示例

### 使用curl命令

#### 创建任务
```bash
curl -X POST http://localhost:8083/api/tasks \
  -H "Content-Type: application/json" \
  -d '{
    "oldPath": "C:/project/old",
    "newPath": "C:/project/new"
  }'
```

响应：
```json
{"taskId":"abc123-def456-ghi789"}
```

#### 查询进度
```bash
curl http://localhost:8083/api/progress/abc123-def456-ghi789
```

响应：
```json
{
  "taskId": "abc123-def456-ghi789",
  "status": "running",
  "progress": {
    "phase": "diff",
    "currentFile": "src/main.go",
    "processedFiles": 5,
    "totalFiles": 20,
    "percent": 25
  }
}
```

#### 获取结果
```bash
curl http://localhost:8083/api/results/abc123-def456-ghi789
```

#### 导出结果
```bash
# 导出JSON格式
curl http://localhost:8083/api/results/abc123-def456-ghi789/export?format=json -o result.json

# 导出CSV格式
curl http://localhost:8083/api/results/abc123-def456-ghi789/export?format=csv -o result.csv
```

## 典型使用场景

### 场景1: Git版本对比
```bash
# 创建两个版本的代码目录
git clone https://github.com/example/project.git old-version
cd old-version && git checkout v1.0 && cd ..

git clone https://github.com/example/project.git new-version
cd new-version && git checkout v2.0 && cd ..

# 使用工具对比
# 在Web界面输入:
# 旧版本路径: ./old-version
# 新版本路径: ./new-version
```

### 场景2: 开发分支对比
```bash
# 准备主分支代码
git clone https://github.com/example/project.git main-branch
cd main-branch && git checkout main && cd ..

# 准备特性分支代码
git clone https://github.com/example/project.git feature-branch
cd feature-branch && git checkout feature/new-feature && cd ..

# 对比分析特性分支的变更
```

### 场景3: 重构前后对比
用于评估代码重构的影响：
- 代码行数变化
- 注释覆盖率变化
- 文件结构调整

## 结果解读指南

### 汇总统计卡片
- **新增总数**: 新增的代码行数 + 注释行数
- **删除总数**: 删除的代码行数 + 注释行数
- **修改总数**: 修改的代码行数 + 注释行数

### 详细统计面板
#### 代码统计
- **净变更**: 新增行数 - 删除行数（正数表示代码增长）
- **变更率**: (新增+删除+修改) / 总行数

#### 注释统计
- **注释比例**: 注释行数 / (代码行数 + 注释行数)
- **注释变更**: 反映文档维护情况

### 图表解读
#### 柱状图 - 代码与注释变更对比
- 高度表示变更数量
- 颜色区分：绿色(新增)、红色(删除)、橙色(修改)
- 对比代码和注释的维护情况

#### 饼图 - 变更类型分布
- 显示新增、删除、修改的比例关系
- 帮助理解变更的主要类型

#### 条形图 - 文件类型统计
- 按文件类型显示变更量
- 了解技术栈的变更分布

### 文件详细列表
- 只显示有变更的文件
- 每个文件的具体变更数据
- 支持按列排序

## 性能优化建议

### 大型项目处理
1. **分模块分析**: 将大项目拆分为多个模块分别分析
2. **排除无关目录**:
   ```
   排除: node_modules/, vendor/, .git/, build/, dist/
   ```
3. **使用相对路径**: 减少路径长度，提高处理速度

### 内存优化
- 关闭不必要的其他应用程序
- 对于超大项目，建议使用16GB+内存的机器

## 常见问题解决

### 问题1: 路径不存在
**错误信息**: "Old path does not exist"
**解决方案**:
- 检查路径拼写是否正确
- 确认目录确实存在
- 使用绝对路径而非相对路径

### 问题2: 权限不足
**现象**: 某些文件读取失败
**解决方案**:
- 以管理员身份运行程序
- 检查目录访问权限

### 问题3: 分析速度慢
**优化方案**:
- 检查是否包含大量二进制文件
- 排除不必要的目录
- 使用SSD硬盘

### 问题4: 内存不足
**现象**: 程序崩溃或响应缓慢
**解决方案**:
- 分批处理大型项目
- 增加系统内存
- 关闭其他内存占用程序

## 最佳实践

### 1. 项目准备
- 确保两个版本的代码结构一致
- 排除临时文件和构建产物
- 统一编码格式(推荐UTF-8)

### 2. 结果分析
- 关注净变更数据，了解实际代码增长
- 对比注释变更，评估文档维护质量
- 分析文件类型分布，了解技术栈变化

### 3. 报告生成
- 导出详细数据进行二次分析
- 结合图表制作项目报告
- 定期对比，建立变更趋势分析

### 4. 团队协作
- 在代码审查中使用统计数据
- 作为开发效率的量化指标
- 识别代码热点和重构需求

---

通过以上示例和指南，你应该能够充分利用代码修改统计系统的各项功能。如有疑问，请参考主README文档或提交Issue。