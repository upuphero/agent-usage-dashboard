import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { LANGUAGE_STORAGE_KEY, localeFor, translate, type Language, type Translator } from './messages';

export function readLanguage(storage?: Pick<Storage, 'getItem'>): Language {
  try {
    const saved = (storage ?? localStorage).getItem(LANGUAGE_STORAGE_KEY);
    return saved === 'en' ? 'en' : 'zh';
  } catch { return 'zh'; }
}

interface I18nValue { language: Language; locale: string; setLanguage: (language: Language) => void; t: Translator }
const I18nContext = createContext<I18nValue>({
  language: 'zh', locale: 'zh-CN', setLanguage: () => {}, t: (key, parameters) => translate('zh', key, parameters),
});
export function I18nProvider({ children, initialLanguage }: { children: ReactNode; initialLanguage?: Language }) {
  const [language, setLanguage] = useState<Language>(() => initialLanguage ?? readLanguage());
  useEffect(() => {
    document.documentElement.lang = localeFor(language);
    document.title = translate(language, 'Agent Usage · 本地用量面板');
    try { localStorage.setItem(LANGUAGE_STORAGE_KEY, language); } catch { /* Switching still works when preference storage is unavailable. */ }
  }, [language]);
  const value = useMemo<I18nValue>(() => ({ language, locale: localeFor(language), setLanguage,
    t: (key, parameters) => translate(language, key, parameters) }), [language]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}
export function useI18n() { return useContext(I18nContext); }
