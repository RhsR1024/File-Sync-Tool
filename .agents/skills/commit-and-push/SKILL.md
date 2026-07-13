---
name: commit-and-push
description: "Use when the current repository has uncommitted project files and the user wants them reviewed, committed, and pushed with a clean working tree."
---

# Commit and Push

Commit the current repository changes and push the active branch.

## Usage

```bash
$commit-and-push
$commit-and-push --message "feat(updater): refine recovery flow"
$commit-and-push --paths src-tauri src --message "fix(sync): narrow touched files"
$commit-and-push --all
```

If `--message` is omitted, generate a concise Conventional Commit message from the dominant diff scope.

## Accepted Parameters

- No arguments
  - Inspect current changes, stage the relevant project files for the current request, generate a commit message, commit, then push.
- `--message "<type>(<scope>): <summary>"`
  - Use the provided commit message exactly as written.
- `--paths <path1> <path2> ...`
  - Restrict staging to the listed repository paths.
- `--all`
  - Stage all repository changes that are safe to include for the current request.

## Parameter Rules

- `--all` and `--paths` are mutually exclusive.
- `--message` may be combined with either default mode, `--paths`, or `--all`.
- Unknown flags should be rejected instead of guessed.
- Unquoted multi-word commit messages should be normalized into a quoted `--message` form before execution.

## Output Contract

- Success:
  - Report the branch, created commit hash, pushed remote ref, and final working-tree status.
- No-op:
  - Report that the repository is already clean and do not create an empty commit.
- Failure:
  - Stop at the failing step and report whether the failure happened during staging, commit, pull, or push.

## Scope

- Include changed project files in the repository.
- If the repository is already clean, report that and stop.

## Steps

1. Inspect the branch and uncommitted changes:
   ```bash
   git status --short --branch
   git diff --stat
   ```
2. Build the staging set:
   - Stage tracked and untracked project files that belong to the current request.
   - Do not stage temp files or unrelated local artifacts just to make the tree clean.
3. Commit the changes:
   ```bash
   git add -A -- <paths>
   git commit -m "<type>(<scope>): <summary>"
   ```
4. Sync and push:
   ```bash
   git pull --ff-only
   git push origin HEAD
   ```
5. Verify the result:
   ```bash
   git status --short --branch
   ```
   Report the new commit hash and confirm whether the working tree is clean.

## Guardrails

- Do not use `git commit --amend` unless the user explicitly asks.
- Do not force-push unless the user explicitly asks.
- If `git pull --ff-only` fails because the remote diverged, stop and report the situation before rewriting history.
- If commit hooks, lint, or tests fail, report the failure and keep the current changes intact for the user.
- If the requested `--paths` set is empty after filtering, stop and report that nothing matched.

## Chinese Examples

```bash
$commit-and-push
```
中文：检查当前仓库的未提交项目文件，自动生成提交信息，提交并推送当前分支。

```bash
$commit-and-push --message "feat(updater): 完善更新恢复流程"
```
中文：使用指定提交信息提交当前请求相关改动，并推送到远端。

```bash
$commit-and-push --paths src-tauri src/components --message "fix(ui): sync updater dialog state"
```
中文：只提交指定目录下的改动，不把仓库里其他无关改动一起带上。

## Common Mistakes

- 把自然语言要求当成提交信息直接使用
  - 应先收敛为 Conventional Commit 风格的 `--message`。
- 为了“清空工作区”把无关临时文件也一起提交
  - 只提交当前请求相关的项目文件。
- `git pull --ff-only` 失败后直接 rebase 或 force-push
  - 应先停止并向用户报告分叉状态。

## Quick Reference

- Default flow: inspect -> stage project files -> commit -> `git pull --ff-only` -> push -> verify clean
- Custom message: accept the user's commit message as-is
- Clean tree: report no-op instead of creating an empty commit
- Safer subset commit: use `--paths`
