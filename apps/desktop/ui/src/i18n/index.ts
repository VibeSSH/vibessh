import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import LanguageDetector from "i18next-browser-languagedetector";
import en from "./locales/en.json";
import pl from "./locales/pl.json";

export const SUPPORTED_LANGUAGES = ["en", "pl"] as const;
export type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number];

/**
 * localStorage (not the OS locale alone) is the primary source, via
 * LanguageDetector's own "localStorage" cache - a real, persisted device
 * preference that survives app restarts, same as VibeSSH's other local-only
 * settings (see Frontend State: device-local UI state doesn't need a backend).
 * navigator falls back only when nothing has been chosen yet.
 */
i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      en: { translation: en },
      pl: { translation: pl },
    },
    fallbackLng: "en",
    supportedLngs: SUPPORTED_LANGUAGES as unknown as string[],
    detection: {
      order: ["localStorage", "navigator"],
      caches: ["localStorage"],
      lookupLocalStorage: "vibessh_language",
    },
    interpolation: { escapeValue: false },
  });

export default i18n;
