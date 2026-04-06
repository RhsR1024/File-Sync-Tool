import assert from 'node:assert/strict';

import * as shareTypes from './types.ts';

assert.equal(
  typeof shareTypes.canRenderEntryThumbnail,
  'function',
  'canRenderEntryThumbnail should be exported',
);

const imageEntry = {
  node_id: 'file-1',
  parent_id: 'dir-1',
  kind: 'file',
  name: 'photo.jpg',
  root_id: 'root-1',
  root_alias: 'soft',
  relative_path: 'DCIM/photo.jpg',
  display_path: 'soft/DCIM/photo.jpg',
  is_dir: false,
  size: 12,
  modified: '2026-04-06 10:00:00',
  permissions: {
    browse: true,
    download_file: true,
    download_archive: false,
    upload_file: false,
    upload_directory: false,
    create_directory: false,
    create_text: false,
    rename: false,
    delete: false,
    preview_image: true,
    search_current: true,
    search_global: true,
  },
};

const enabledSession = {
  username: 'guest',
  is_guest: true,
  permissions: imageEntry.permissions,
  features: {
    image_preview_enabled: true,
    thumbnail_enabled: true,
  },
};

assert.equal(
  shareTypes.canRenderEntryThumbnail(enabledSession, imageEntry),
  true,
  'image entries should render thumbnails when both feature flags are enabled',
);

assert.equal(
  shareTypes.canRenderEntryThumbnail(
    {
      ...enabledSession,
      features: {
        ...enabledSession.features,
        thumbnail_enabled: false,
      },
    },
    imageEntry,
  ),
  false,
  'thumbnail toggle should disable list thumbnails',
);

assert.equal(
  shareTypes.canRenderEntryThumbnail(
    {
      ...enabledSession,
      features: {
        ...enabledSession.features,
        image_preview_enabled: false,
      },
    },
    imageEntry,
  ),
  false,
  'image preview toggle should also disable list thumbnails',
);

assert.equal(
  shareTypes.canRenderEntryThumbnail(enabledSession, {
    ...imageEntry,
    name: 'readme.txt',
  }),
  false,
  'non-image files should not render thumbnails',
);

const noPreviewPermissionEntry = {
  ...imageEntry,
  permissions: {
    ...imageEntry.permissions,
    preview_image: false,
  },
};

assert.equal(
  shareTypes.canRenderEntryThumbnail(enabledSession, noPreviewPermissionEntry),
  false,
  'preview permission should still gate list thumbnails',
);

assert.equal(
  shareTypes.canRenderEntryThumbnail(null, imageEntry),
  false,
  'missing session data should disable thumbnails defensively',
);

assert.equal(
  shareTypes.canRenderEntryThumbnail(enabledSession, {
    ...imageEntry,
    kind: 'directory',
    is_dir: true,
  }),
  false,
  'directories should never render image thumbnails',
);

assert.equal(
  shareTypes.formatFileSize(0),
  '0 B',
  'zero-byte files should render a real size instead of the unknown placeholder',
);

assert.equal(
  shareTypes.formatFileSize(null),
  '-',
  'missing sizes should still render the unknown placeholder',
);

console.log('share-web types tests PASSED');
