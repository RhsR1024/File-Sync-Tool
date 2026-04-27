# Error Code Git2 Sync Design

- **Date**: 2026-04-27
- **Status**: Approved
- **Owner**: codex-agent

---

## Goal

Replace the current error-code sync transport with an embedded Git-based implementation that does not require Git to be installed on the user's Windows machine.

## Constraints

- Continue using the existing GitLab username and password.
- Do not require a GitLab token.
- Do not require `git.exe` on the target machine.
- Keep the existing cache format, CSV parser, query API, and UI unchanged.
- Preserve the existing `main` then `master` fallback behavior.

## Design

### Transport

Use the Rust `git2` crate with vendored libgit2/OpenSSL support so the app carries its own Git transport stack.

The sync flow will:

1. Build the repository URL `http://igcode.uniview.com/RD-UNIVIEW/public/pubResList/errorcode.git`.
2. Try branch `main`, then fallback branch `master`.
3. Clone into a temporary directory with shallow depth `1`.
4. Authenticate through `RemoteCallbacks::credentials` using the configured username and password.
5. Walk the checked-out worktree and collect `*.csv` files by basename.
6. Reuse the existing parser, cache writer, and in-memory store update flow.
7. Remove the temporary clone directory after completion or failure.

### Error handling

- Authentication failures stay mapped to `SyncError::Auth`.
- Branch-not-found on `main` falls through to `master`.
- Clone/fetch/worktree traversal failures map to `SyncError::Network` or `SyncError::Io` with enough log detail to diagnose the failing step.
- "No CSV found" remains an archive/store-level error even though the transport is no longer a zip archive.

### Logging

Keep the existing tool log style, but update messages to reflect Git transport steps:

- starting Git sync
- trying branch
- clone success/failure
- CSV file count
- final source branch

### Testing

- Unit test: collect CSV files from a checked-out directory and ignore non-CSV files.
- Integration-style unit test with a local temporary Git repository:
  - sync succeeds when only `main` exists
  - sync falls back from `main` to `master`

## Out of scope

- UI changes
- token-based auth
- incremental sync
- keeping a long-lived local mirror repository
