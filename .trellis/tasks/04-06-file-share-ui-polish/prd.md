# File Share UI Polish

## Goal
Polish the file share experience by improving directory breadcrumbs, adding list thumbnails for image files, and fixing settings-page layout issues.

## Requirements
- Show hierarchical breadcrumbs in the share-web toolbar, using the existing tree breadcrumb data.
- Add a small leading thumbnail for image files in the share-web entry list.
- Make the existing `thumbnail_enabled` setting actually control whether list thumbnails render.
- Keep `image_preview_enabled` as the master gate for preview access; when disabled, thumbnails must not render.
- Fix the password input layout on the file share settings page so text does not visually overlap.
- Clarify the shared-root path action as editing the configured root path, and prevent its button text from wrapping.

## Acceptance Criteria
- [ ] Share-web displays breadcrumbs as a readable path hierarchy instead of chip-only navigation.
- [ ] Image entries show a leading thumbnail when thumbnails are enabled and preview is allowed.
- [ ] Disabling thumbnails removes list thumbnails without breaking the existing preview dialog.
- [ ] Disabling image preview also disables list thumbnails.
- [ ] File share settings page password inputs render cleanly without overlap on the current layout.
- [ ] Path action buttons and other affected buttons stay on one line and look balanced.

## Technical Notes
- This task touches the settings page, Rust file-share runtime/API contract, and the share-web frontend.
- The share-web client will need a runtime flag from the backend to know whether thumbnails are enabled.
- Work must preserve existing in-progress tree-unification changes in the dirty worktree.
