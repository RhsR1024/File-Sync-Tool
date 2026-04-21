<!-- TRELLIS:START -->
# Trellis Instructions

These instructions are for AI assistants working in this project.

Use the `/trellis:start` command when starting a new session to:
- Initialize your developer identity
- Understand current project context
- Read relevant guidelines

Use `@/.trellis/` to learn:
- Development workflow (`workflow.md`)
- Project structure guidelines (`spec/`)
- Developer workspace (`workspace/`)

Keep this managed block so 'trellis update' can refresh the instructions.

<!-- TRELLIS:END -->

## Worktree Shared Dependencies

- When using a git worktree under this repository, prefer sharing dependencies and build artifacts with the main workspace instead of creating duplicate large directories.
- Frontend dependencies: point the worktree `node_modules` to the repo-root `node_modules` with a junction or symlink when possible.
- Rust and Tauri builds: set `CARGO_TARGET_DIR` to the repo-root `src-tauri/target` or another shared target directory when running cargo commands from a worktree.
- If a shared dependency path is recreated or goes missing, verify `vite`, `vue-tsc`, and cargo artifacts resolve correctly before treating it as a broken environment.
- Do not commit temporary dependency copies that were created only to work around worktree isolation.
