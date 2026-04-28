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

## Modes

- Default mode: keep local commits and working tree edits by rebasing with autostash when needed.
- `--force`: replace tracked files with the upstream branch state.

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

## Quick Reference

- Keep local work: `git pull --rebase --autostash`
- Force overwrite tracked files: `git fetch --all --prune` then `git reset --hard @{u}`
- Remove untracked files: only after explicit approval for extra cleanup
