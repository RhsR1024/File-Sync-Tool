# 项目规则

## 开发命令

- **前端开发**: `pnpm dev`
- **后端开发**: `pnpm tauri dev`
- **构建**: `pnpm tauri build`

# 开发规则

## 国际化 (i18n)
1. **双语支持**: 所有面向用户的文本必须同时支持英语 (en) 和中文 (zh)。
2. **键值一致性**: 新功能必须在 `src/locales/messages.ts` 中为两种语言添加对应的键。
3. **避免硬编码**: 不要在 Vue 组件中硬编码文本。使用 `t('key')` 或 `t('key') || 'Fallback'` 模式（建议仅使用键名）。

## 代码风格
1. **Vue Composition API**: 使用 `<script setup>` 和 Composition API。
2. **Tailwind CSS**: 使用 Tailwind 工具类进行样式设计。
3. **类型安全**: 为数据结构使用 TypeScript 接口（例如 `AppConfig`, `HistoryEntry`）。

## Git 工作流
1. **提交信息**: 使用中文（中文）编写描述性的提交信息。
2. **保持整洁**: 提交前尽可能确保没有未使用的导入或警告。

## 交互输出
1. **语言要求**: 回答输出需要使用中文。
