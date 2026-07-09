<!-- TRELLIS:START -->
# Trellis Instructions

These instructions are for AI assistants working in this project.

This project is managed by Trellis. The working knowledge you need lives under `.trellis/`:

- `.trellis/workflow.md` — development phases, when to create tasks, skill routing
- `.trellis/spec/` — package- and layer-scoped coding guidelines (read before writing code in a given layer)
- `.trellis/workspace/` — per-developer journals and session traces
- `.trellis/tasks/` — active and archived tasks (PRDs, research, jsonl context)

If a Trellis command is available on your platform (e.g. `/trellis:finish-work`, `/trellis:continue`), prefer it over manual steps. Not every platform exposes every command.

If you're using Codex or another agent-capable tool, additional project-scoped helpers may live in:
- `.agents/skills/` — reusable Trellis skills
- `.codex/agents/` — optional custom subagents

Managed by Trellis. Edits outside this block are preserved; edits inside may be overwritten by a future `trellis update`.

<!-- TRELLIS:END -->

## Worktree Shared Dependencies

- When using a git worktree under this repository, prefer sharing dependencies and build artifacts with the main workspace instead of creating duplicate large directories.
- Frontend dependencies: point the worktree `node_modules` to the repo-root `node_modules` with a junction or symlink when possible.
- Rust and Tauri builds: set `CARGO_TARGET_DIR` to the repo-root `src-tauri/target` or another shared target directory when running cargo commands from a worktree.
- If a shared dependency path is recreated or goes missing, verify `vite`, `vue-tsc`, and cargo artifacts resolve correctly before treating it as a broken environment.
- Do not commit temporary dependency copies that were created only to work around worktree isolation.
