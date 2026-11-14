import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { api } from "./api";
import { applyLanguage, isLang, systemLanguage, type Lang } from "./i18n";

type LanguageState = { language: Lang; chooseLanguage: (lang: Lang) => void };

const LanguageContext = createContext<LanguageState>({ language: "it", chooseLanguage: () => {} });

/**
 * La lingua scelta nelle Impostazioni, salvata nel nucleo. Chi la legge con
 * `useLanguage()` si ridisegna quando cambia: `App` lo fa, e con lei tutta
 * l'interfaccia.
 */
export function LanguageProvider({ children }: { children: ReactNode }) {
  const [language, setLanguage] = useState<Lang>(() => {
    const initial = systemLanguage();
    applyLanguage(initial);
    return initial;
  });

  useEffect(() => {
    api
      .language()
      .then((saved) => {
        if (isLang(saved)) {
          applyLanguage(saved);
          setLanguage(saved);
        }
      })
      .catch(() => {});
  }, []);

  const chooseLanguage = useCallback((lang: Lang) => {
    applyLanguage(lang);
    setLanguage(lang);
    api.setLanguage(lang).catch(() => {});
  }, []);

  return <LanguageContext.Provider value={{ language, chooseLanguage }}>{children}</LanguageContext.Provider>;
}

export const useLanguage = () => useContext(LanguageContext);
