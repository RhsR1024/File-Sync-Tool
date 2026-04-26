/// <reference types="vite/client" />

// Build-time injected by Vite `define` (see `vite.config.ts`).
// Acts as the structured source for the current release date so pages don't
// have to parse the i18n `sidebar.version` string.
declare const __APP_RELEASE_DATE__: string;
