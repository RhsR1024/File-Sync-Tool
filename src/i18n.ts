import { createI18n } from 'vue-i18n';
import { messages } from './locales/messages';

const defaultLocale = localStorage.getItem('locale') || (navigator.language.startsWith('en') ? 'en' : 'zh');

export const i18n = createI18n({
  legacy: false, // Use Composition API mode
  locale: defaultLocale,
  fallbackLocale: 'zh',
  messages,
});
