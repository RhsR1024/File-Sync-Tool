---
name: update-from-remote
description: "Use when the user wants to sync the current repository with its upstream branch, preserving local changes by default and allowing explicit overwrite with --force."
---

# Update From Remote

Update the current repository from its upstream branch.

## Usage

```bash
$update-from-remote
$update-from-remote --force
```

Default behavior preserves local work. `--force` is destructive and only applies when the user explicitly asks to overwrite tracked files with upstream state.

## Accepted Parameters

- No arguments
  - Update from the configured upstream branch while preserving local modifications.
- `--force`
  - Overwrite tracked files with upstream state after fetching.

## Parameter Rules

- The canonical forms are only `$update-from-remote` and `$update-from-remote --force`.
- Unknown flags should be rejected instead of guessed.
- `--force` only changes tracked files; it does not remove untracked files by default.
- If the branch has no upstream, stop before any pull, fetch, or reset step that assumes tracking exists.

## Modes

- Default mode: keep local commits and working tree edits by rebasing with autostash when needed.
- `--force`: replace tracked files with the upstream branch state.

## Output Contract

- Success:
  - Report the upstream branch, whether local edits were preserved or overwritten, and the final working-tree status.
- Failure:
  - Stop and report the exact failing stage: upstream lookup, pull/rebase, conflict, fetch, or reset.
- Partial update:
  - If upstream fetch succeeds but rebase conflicts, keep the repository intact and report the conflicted files.

## Steps

1. Inspect the current state:
   ```bash
   git status --short --branch
   git rev-parse --abbrev-ref --symbolic-full-name @{u}
   ```
2. Default mode:
   - If the tree is clean:
     ```bash
     git pull --rebase
     ```
   - If the tree is dirty:
     ```bash
     git pull --rebase --autostash
     ```
   - If replay causes conflicts, stop and report the conflicted files.
3. Force mode (`--force`):
   - Confirm the user explicitly requested overwrite.
   - Replace tracked files with upstream state:
     ```bash
     git fetch --all --prune
     git reset --hard @{u}
     ```
   - Leave untracked files alone unless the user also explicitly asks to remove them.
4. Verify the result:
   ```bash
   git status --short --branch
   ```
   Report whether the tree is clean or still dirty because local edits were preserved.

## Guardrails

- Default mode must not discard local changes.
- Never run `git reset --hard` unless the user explicitly requested `--force`.
- If there is no upstream branch, stop and report the missing tracking configuration.
- If the user wants an exact remote mirror, ask whether untracked files should also be removed.
- If rebase conflicts appear in default mode, do not auto-resolve them.

## Chinese Examples

```bash
$update-from-remote
```
中文：从当前分支的上游拉取最新代码，默认保留本地未提交修改，必要时使用 `autostash`。

```bash
$update-from-remote --force
```
中文：明确按远端状态覆盖当前分支的已跟踪文件，但默认不删除未跟踪文件。

## Common Mistakes

- 把默认模式误解成“强制同步”
  - 默认模式的目标是保留本地工作，不是覆盖它。
- 认为 `--force` 会顺便清理未跟踪文件
  - 它只覆盖已跟踪文件；删除未跟踪文件需要用户再次明确授权。
- 发生 rebase 冲突后继续自动推进
  - 应停止并把冲突文件报告给用户。

## Quick Reference

- Keep local work: `git pull --rebase --autostash`
- Force overwrite tracked files: `git fetch --all --prune` then `git reset --hard @{u}`
- Remove untracked files: only after explicit approval for extra cleanup
- Reject unknown flags instead of inferring intent
