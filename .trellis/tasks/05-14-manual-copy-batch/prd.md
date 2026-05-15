# Manual Copy — Batch Source with Auto-Disambiguation

## Goal

让"指定复制"（`ManualCopyModal`）支持一次粘贴多条版本路径并自动入队，当多条源末尾目录同名时按"递归向上"算法把消歧段插入到目标路径里，避免后入队的任务把先入队的目标覆盖掉。

## Worked example

目标根：`E:\UMS_TEMP\1.3.9.P10`，源路径粘贴：

```
\\nt03\iCPD\版本\UMS\正式版本\V100R001B02\1.3.9.P10
\\nt03\iCPD\版本\UMS\正式版本\V100R001B08\1.3.9.P10
\\nt03\iCPD\版本\VMS\正式版本\V200R001B01\1.3.9.P10
\\nt03\iCPD\版本\UMS-IPSAN\1.3.9.P10
...
```

→ 自动解析为各自独立的最终目标：

```
E:\UMS_TEMP\1.3.9.P10\V100R001B02\1.3.9.P10
E:\UMS_TEMP\1.3.9.P10\V100R001B08\1.3.9.P10
E:\UMS_TEMP\1.3.9.P10\V200R001B01\1.3.9.P10
E:\UMS_TEMP\1.3.9.P10\UMS-IPSAN\1.3.9.P10
...
```

## Spec

完整设计见 [docs/superpowers/specs/2026-05-14-manual-copy-batch-design.md](../../../docs/superpowers/specs/2026-05-14-manual-copy-batch-design.md)。

## Acceptance Criteria

- [ ] 单行粘贴时模态行为与今天完全一致（无回归）。
- [ ] 多行粘贴（≥2 行有效路径）进入批量模式，"开始复制"按钮变为"预览批次 (N)"。
- [ ] 预览表显示每条的源路径 / 算出的最终目标 / 状态徽章；OK 行默认勾选，问题行默认不勾选但可手动勾上。
- [ ] 消歧算法在用户给出的 9 条 UMS/VMS 路径上得到验证表里完全一致的最终目标。
- [ ] 倒数第二段也撞时，递归继续向上走一段直到唯一；无法消歧时标为"批次内重复"。
- [ ] 提交按勾选顺序串行调用 `queueTemporaryCopy`，任一行失败不影响其他行的入队。
- [ ] 失败汇总用 toast 显示 `成功入队 X/Y，失败行请修正后重试`。
- [ ] 算法模块有独立 vitest（覆盖至少 8 个场景，见 spec § 8）。
- [ ] i18n key 同时新增 en + zh。
- [ ] 后端零改动。

## Branching

直接在 `main` 上实施（per user preference for small UX tweaks）。

## Files Touched

新增：
- `src/lib/manualCopyBatch.ts`
- `src/lib/__tests__/manualCopyBatch.test.ts`

修改：
- `src/components/ManualCopyModal.vue`
- `src/locales/messages.ts`
- 现有模态/页面测试（如需）
