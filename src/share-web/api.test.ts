import { describe, expect, it } from 'vitest';

import { fileShareApi } from './api';

describe('fileShareApi download URLs', () => {
  it('builds one archive URL for a multi-node selection', () => {
    expect(fileShareApi.downloadSelectionArchiveUrl(['file.root-1.a', 'file.root-1.b'])).toBe(
      '/api/download/selection-archive?node_id=file.root-1.a&node_id=file.root-1.b',
    );
  });
});
