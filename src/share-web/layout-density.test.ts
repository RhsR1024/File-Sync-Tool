import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const __dirname = dirname(fileURLToPath(import.meta.url));

describe('file share web layout density', () => {
  it('keeps breadcrumbs in the top bar and folds directory context into the search row', () => {
    const appSource = readFileSync(join(__dirname, 'App.vue'), 'utf8');
    const topBarSource = readFileSync(join(__dirname, 'components', 'TopBar.vue'), 'utf8');
    const searchBarSource = readFileSync(join(__dirname, 'components', 'SearchBar.vue'), 'utf8');
    const styleSource = readFileSync(join(__dirname, 'style.css'), 'utf8');

    expect(appSource).toMatch(/<TopBar[\s\S]*:breadcrumbs="breadcrumbs"[\s\S]*@navigate="navigate"/);
    expect(appSource).not.toMatch(/:page-title="pageTitle"/);
    expect(appSource).not.toMatch(/@navigate-up="navigateUp"/);
    expect(appSource).not.toMatch(/<div class="page-head">/);
    expect(appSource).toMatch(/<template #leading>[\s\S]*class="toolbar-context-row"[\s\S]*class="btn toolbar-back"[\s\S]*class="toolbar-title"/);
    expect(appSource).toMatch(/<SearchBar[\s\S]*<template #actions>[\s\S]*<ToolbarActions/);
    expect(searchBarSource).toMatch(/<slot name="leading" \/>/);
    expect(searchBarSource).toMatch(/<slot name="actions" \/>/);
    expect(topBarSource).toMatch(/<Breadcrumbs[\s\S]*:breadcrumbs="breadcrumbs"/);
    expect(topBarSource).not.toMatch(/topbar-title-row|topbar-back|topbar-title/);

    const mainBlock = styleSource.match(/\.main\s*\{[\s\S]*?\n\}/)?.[0] ?? '';
    const topbarContextBlock = styleSource.match(/\.topbar-context\s*\{[\s\S]*?\n\}/)?.[0] ?? '';
    const toolbarContextBlock = styleSource.match(/\.toolbar-context-row\s*\{[\s\S]*?\n\}/)?.[0] ?? '';
    const searchBlock = styleSource.match(/\.search\s*\{[\s\S]*?\n\}/)?.[0] ?? '';
    const toolbarActionsBlock = styleSource.match(/\.toolbar \.page-actions\s*\{[\s\S]*?\n\}/)?.[0] ?? '';

    expect(mainBlock).toMatch(/padding:\s*12px 28px 60px/);
    expect(mainBlock).toMatch(/gap:\s*12px/);
    expect(topbarContextBlock).toMatch(/flex-direction:\s*row/);
    expect(topbarContextBlock).toMatch(/flex:\s*1 1 auto/);
    expect(toolbarContextBlock).toMatch(/flex:\s*0 1 auto/);
    expect(searchBlock).toMatch(/flex:\s*1 1 320px/);
    expect(toolbarActionsBlock).toMatch(/margin-left:\s*auto/);
  });
});
