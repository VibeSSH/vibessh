import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Card } from "@/components/ui/Card";
import { getAppInfo } from "@/services/appService";
import { SUPPORTED_LANGUAGES, type SupportedLanguage } from "@/i18n";
import "./pages.css";
import "./Settings.css";

const LANGUAGE_LABEL_KEY: Record<SupportedLanguage, string> = {
  en: "settings.languageEnglish",
  pl: "settings.languagePolish",
};

export function Settings() {
  const { t, i18n } = useTranslation();
  const [appInfo, setAppInfo] = useState<{ name: string; version: string } | null>(null);

  useEffect(() => {
    getAppInfo()
      .then(setAppInfo)
      .catch(() => setAppInfo(null));
  }, []);

  return (
    <div className="page">
      <div className="page-header">
        <h1 className="page-title">{t("settings.title")}</h1>
        <p className="page-subtitle">{t("settings.subtitle")}</p>
      </div>

      <Card title={t("settings.preferences")} subtitle={t("settings.preferencesSubtitle")}>
        <div className="settings-preference-row">
          <div>
            <p className="settings-preference-label">{t("settings.language")}</p>
            <p className="settings-muted">{t("settings.languageDescription")}</p>
          </div>
          <div className="settings-language-switch">
            {SUPPORTED_LANGUAGES.map((lang) => (
              <button
                key={lang}
                className={`settings-language-btn ${i18n.resolvedLanguage === lang ? "settings-language-btn-active" : ""}`}
                onClick={() => i18n.changeLanguage(lang)}
                aria-pressed={i18n.resolvedLanguage === lang}
              >
                {t(LANGUAGE_LABEL_KEY[lang])}
              </button>
            ))}
          </div>
        </div>
      </Card>

      <Card title={t("settings.about")} subtitle={t("settings.aboutSubtitle")}>
        {appInfo ? (
          <p className="settings-row">
            {appInfo.name} <span className="settings-muted">v{appInfo.version}</span>
          </p>
        ) : (
          <p className="settings-muted">{t("settings.backendWaiting")}</p>
        )}
      </Card>
    </div>
  );
}
