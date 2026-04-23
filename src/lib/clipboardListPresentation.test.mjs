import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  buildClipboardHighlightParts,
  extractClipboardSearchKeywords,
  resolveClipboardSourceBadge,
  resolveSourceAppPresentation,
} from './clipboardListPresentation.ts';

test('extractClipboardSearchKeywords keeps only free-text keywords from the clipboard DSL', () => {
  assert.deepEqual(
    extractClipboardSearchKeywords('type:image app:code fav:true release notes bugfix'),
    ['release', 'notes', 'bugfix'],
  );
});

test('buildClipboardHighlightParts marks case-insensitive keyword matches without dropping surrounding text', () => {
  assert.deepEqual(
    buildClipboardHighlightParts('Release notes and bugfix follow-up', ['notes', 'BUGFIX']),
    [
      { text: 'Release ', match: false },
      { text: 'notes', match: true },
      { text: ' and ', match: false },
      { text: 'bugfix', match: true },
      { text: ' follow-up', match: false },
    ],
  );
});

test('resolveSourceAppPresentation follows the configured source-app display mode', () => {
  assert.deepEqual(resolveSourceAppPresentation('both', 'Code', 'C:/icons/code.png'), {
    showIcon: true,
    showName: true,
  });
  assert.deepEqual(resolveSourceAppPresentation('icon', 'Code', null), {
    showIcon: true,
    showName: false,
  });
  assert.deepEqual(resolveSourceAppPresentation('name', 'Code', 'C:/icons/code.png'), {
    showIcon: false,
    showName: true,
  });
  assert.deepEqual(resolveSourceAppPresentation('none', 'Code', 'C:/icons/code.png'), {
    showIcon: false,
    showName: false,
  });
});

test('resolveClipboardSourceBadge prefers the self-tool badge over app metadata', () => {
  assert.deepEqual(
    resolveClipboardSourceBadge('both', {
      from_self: true,
      source_app: 'Code',
      source_app_icon: 'C:/icons/code.png',
    }),
    {
      kind: 'self',
      showIcon: true,
      showName: true,
      label: 'This tool',
    },
  );
});
