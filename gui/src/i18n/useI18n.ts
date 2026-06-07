import { useState, useCallback } from 'react';
import { Lang, languages, tr as translate } from './index';

export function useI18n() {
  const [lang, setLang] = useState<Lang>(() => {
    try { return (localStorage.getItem('lang') as Lang) || 'en'; } catch { return 'en'; }
  });

  const setLanguage = useCallback((l: Lang) => {
    setLang(l);
    try { localStorage.setItem('lang', l); } catch {}
  }, []);

  const t = useCallback((key: string) => translate(lang, key), [lang]);

  return { lang, setLanguage, t, languages };
}
