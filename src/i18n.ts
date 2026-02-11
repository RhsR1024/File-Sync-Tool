import { createI18n } from 'vue-i18n';
import { messages } from './locales/messages';

// Get system language or default to zh
const getBrowserLanguage = () => {
  const lang = navigator.language;
  if (lang.startsWith('zh')) {
    return 'zh';
  }
  return 'en'; // Default to English if not Chinese, or change logic as needed. 
               // Requirement says "default Chinese OR read system language".
               // If we want to strictly follow "default Chinese", we could just return 'zh' if system lang detection fails.
               // Here: System zh -> zh, others -> en. But user asked "default Chinese or read system".
               // Let's refine: If system is zh, use zh. Else if system is en, use en. Fallback to zh.
};

const defaultLocale = localStorage.getItem('locale') || (navigator.language.startsWith('en') ? 'en' : 'zh');

export const i18n = createI18n({
  legacy: false, // Use Composition API mode
  locale: defaultLocale,
  fallbackLocale: 'zh',
  messages,
});
