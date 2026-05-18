# Clipboard Manager

> Contracts for clipboard history groups, active capture target, and group-scoped mutations.

## Scenario: Group Semantics Parity

### 1. Scope / Trigger

- Trigger: Code that lists clipboard items, captures new clipboard content, clears history, deletes groups, or moves items between groups.
- Goal: Match ElegantClipboard group semantics. `NULL` group id is the default ungrouped bucket, not an "all groups" view.

### 2. Signatures

- Tauri command: `cb_set_active_group(groupId: number | null) -> void`
- Tauri command: `cb_list(query: ClipboardListQuery) -> ClipboardListResult`
- Tauri command: `cb_clear(keepFavorites: boolean, groupId: number | null) -> number`
- Tauri command: `cb_clear_all(keepFavorites: boolean) -> number`
- Tauri command: `cb_groups_delete(id: number) -> void`
- Tauri command: `cb_move_to_group(itemId: number, groupId: number | null) -> ClipboardItem`
- DB: `clipboard_items.group_id INTEGER REFERENCES clipboard_groups(id) ON DELETE CASCADE`
- DB: `clipboard_items.hash` is not globally unique; duplicate hashes may exist in different groups.

### 3. Contracts

- `groupId = null` means the default ungrouped bucket (`group_id IS NULL`).
- `groupId = <id>` means the custom group with that id (`group_id = <id>`).
- Item list queries must always apply the active group filter. There is no implicit "all groups" result when `groupId` is null.
- New captures must read `ClipboardState.active_group_id` and insert/upsert into that group.
- Deduplication is group-scoped: the same hash is a duplicate only inside the same group.
- Deleting a custom group deletes all clipboard items in that group through the FK cascade and then runs asset cleanup.
- `cb_clear` clears only the selected/default group. Settings-level full cleanup must call `cb_clear_all`.
- Group list responses include `item_count` so the UI can show destructive delete scope.

### 4. Validation & Error Matrix

| Operation | Valid Input | Error / Edge |
|-----------|-------------|--------------|
| `cb_set_active_group` | `null` or existing group id | If a stale id is set, future inserts may fail FK validation; frontend must reset after group list refresh. |
| `cb_list` | `groupId: null` | Returns only ungrouped rows. |
| `cb_list` | `groupId: 7` | Returns only rows with `group_id = 7`. |
| `cb_groups_delete` | Existing group id | Deletes group rows and cascades items; missing id returns `clipboard group not found`. |
| `cb_clear` | `keepFavorites=true`, `groupId=7` | Deletes non-favorites only in group 7. |
| `cb_clear_all` | `keepFavorites=false` | Deletes all clipboard rows across all groups. |

### 5. Good/Base/Bad Cases

- Good: User selects group `Work`, takes a screenshot, and the new image row has `group_id = Work.id`.
- Good: The same copied text can exist once in `Default` and once in `Work`.
- Base: User selects `Default`; list and new captures use `group_id IS NULL`.
- Bad: Passing `groupId = null` to `cb_list` returns rows from every group.
- Bad: Deleting a custom group sets item `group_id` to null and leaves the rows in history.

### 6. Tests Required

- Rust: `clipboard::db::tests::default_group_filter_returns_only_ungrouped_items`
  - Assert `ClipboardListQuery { group_id: None }` returns only `group_id IS NULL`.
- Rust: `clipboard::db::tests::duplicate_hashes_are_allowed_across_groups`
  - Assert the same hash can be inserted into a custom group and default group.
- Rust: `clipboard::db::tests::group_crud_and_item_moves_work`
  - Assert deleting a group removes its items.
- Frontend type tests:
  - Assert `ClipboardGroup` includes `item_count`.

### 7. Wrong vs Correct

#### Wrong

```rust
// Treats default as "all groups" and loses active group semantics.
if let Some(group_id) = q.group_id {
    clauses.push("group_id = ?".into());
}
```

#### Correct

```rust
match q.group_id {
    Some(group_id) => {
        clauses.push("group_id = ?".into());
        values.push(Box::new(group_id));
    }
    None => clauses.push("group_id IS NULL".into()),
}
```
