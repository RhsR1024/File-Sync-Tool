import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const projectRoot = process.cwd();
const screenShareIndex = readFileSync(
  resolve(projectRoot, 'src/screen-share-web/index.html'),
  'utf8',
);
const screenShareFavicon = readFileSync(
  resolve(projectRoot, 'src/screen-share-web/screen-share-favicon.svg'),
  'utf8',
);
const appIndex = readFileSync(resolve(projectRoot, 'index.html'), 'utf8');

describe('screen share favicon', () => {
  it('uses the dedicated screen-share icon instead of the retired project favicon', () => {
    expect(screenShareIndex).toContain('href="./screen-share-favicon.svg"');
    expect(screenShareIndex).not.toContain('href="./favicon.svg"');
    expect(screenShareFavicon).toContain('stroke="#4F46E5"');
    expect(appIndex).not.toContain('href="/favicon.svg"');
    expect(existsSync(resolve(projectRoot, 'src/screen-share-web/favicon.svg'))).toBe(false);
    expect(existsSync(resolve(projectRoot, 'public/favicon.svg'))).toBe(false);
  });
});
