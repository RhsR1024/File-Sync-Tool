# Development Rules

## Internationalization (i18n)
1. **Bilingual Support**: All user-facing text must support both English (en) and Chinese (zh).
2. **Key Consistency**: New features must add corresponding keys to `src/locales/messages.ts` for both languages.
3. **Avoid Hardcoding**: Do not hardcode text in Vue components. Use `t('key')` or `t('key') || 'Fallback'` pattern (though pure keys are preferred).

## Code Style
1. **Vue Composition API**: Use `<script setup>` and Composition API.
2. **Tailwind CSS**: Use Tailwind utility classes for styling.
3. **Type Safety**: Use TypeScript interfaces for data structures (e.g. `AppConfig`, `HistoryEntry`).

## Git Workflow
1. **Commit Messages**: Use descriptive commit messages in Chinese (中文).
2. **Clean State**: Ensure no unused imports or warnings before committing when possible.
