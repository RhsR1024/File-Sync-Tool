import { createI18n } from 'vue-i18n';

import { messages } from './messages';

const locale = navigator.language?.toLowerCase().startsWith('zh') ? 'zh' : 'en';

export const i18n = createI18n({
  legacy: false,
  locale,
  fallbackLocale: 'en',
  messages,
});
