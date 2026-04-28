---
name: commit-and-push
description: "Use when the current repository has uncommitted project files and the user wants them reviewed, committed, and pushed with a clean working tree."
---

# Commit and Push

Commit the current repository changes and push the active branch.

## Usage

```bash
$commit-and-push
$commit-and-push feat(updater): refine recovery flow
```

If the user provides a commit message, use it. Otherwise, generate a concise Conventional Commit message from the dominant diff scope.

## Scope

- Include changed project files in the repository.
- Exclude personal session artifacts such as `.trellis/workspace/` unless the user explicitly asks to include them.
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

## Quick Reference

- Default flow: inspect -> stage project files -> commit -> `git pull --ff-only` -> push -> verify clean
- Custom message: accept the user's commit message as-is
- Clean tree: report no-op instead of creating an empty commit
