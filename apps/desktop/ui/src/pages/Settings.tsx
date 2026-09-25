import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-shell";
import { Button } from "@/components/ui/Button";
import { Details } from "@/components/ui/Details";
import { Card } from "@/components/ui/Card";
import { ThemePicker } from "@/components/settings/ThemePicker";
import { Checkbox } from "@/components/ui/Checkbox";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { Select } from "@/components/ui/Select";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { Switch } from "@/components/ui/Switch";
import { useModalDialog } from "@/hooks/useModalDialog";
import { AiUsageModal } from "@/components/ai/AiUsageModal";
import { getAiConfig, getAiQuota, setAiConfig, testAiConnection } from "@/services/aiService";
import { getAppInfo } from "@/services/appService";
import { McpCard } from "@/components/settings/McpCard";
import { getTraySettings, setMinimizeToTray } from "@/services/trayService";
import { getBackupDestination, setBackupDestination, testBackupDestination } from "@/services/applicationBackupService";
import { listRegistryCredentials, removeRegistryCredential, setRegistryCredential } from "@/services/applicationService";
import { getDnsSuffix, setDnsSuffix } from "@/services/networkService";
import { toastSuccess } from "@/stores/toastStore";
import type { AiConfigView, AiProviderKind, AiQuota } from "@/types/ai";
import type { BackupDestinationConfig, RegistryCredential } from "@/types/application";
import { SUPPORTED_LANGUAGES, type SupportedLanguage } from "@/i18n";
import "./pages.css";
import "./Settings.css";
import "@/components/servers/forms.css";
import "@/components/servers/AddServerModal.css";
import { cloudBackendIsConfigured, cloudGetBackendUrl, cloudSetBackendUrl } from "@/services/cloudService";
import { GuideLink } from "@/guide/GuideLink";
import { CommandError, errorMessage } from "@/services/tauri";

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

        <div className="settings-preference-row settings-preference-row-stacked">
          <div>
            <p className="settings-preference-label">{t("theme.title")}</p>
            <p className="settings-muted">{t("theme.description")}</p>
          </div>
          <ThemePicker />
        </div>

        <MinimizeToTrayRow />
      </Card>

      <AiCard />

      <McpCard />

      <BackupDestinationCard />

      <RegistryCredentialsCard />

      <CloudBackendCard />

      <DnsSuffixCard />

      <AboutCard appInfo={appInfo} />
    </div>
  );
}

const AUTHOR_NAME = "Krystian Michalski";
const AUTHOR_EMAIL = "kryspekxd@gmail.com";
const WEBSITE_URL = "https://vibessh.dev";

/** Who made VibeSSH and how to reach them, under the version it reports. */
function AboutCard({ appInfo }: { appInfo: { name: string; version: string } | null }) {
  const { t } = useTranslation();

  async function copyEmail() {
    try {
      await navigator.clipboard.writeText(AUTHOR_EMAIL);
      toastSuccess(t("settings.aboutEmailCopied"));
    } catch (err) {
      console.warn("copying the contact address failed", err);
    }
  }

  return (
    <Card title={t("settings.about")} subtitle={t("settings.aboutSubtitle")}>
      <dl className="settings-about">
        <dt>{t("settings.aboutVersion")}</dt>
        <dd>
          {appInfo ? (
            <>
              {appInfo.name} <span className="settings-muted">v{appInfo.version}</span>
            </>
          ) : (
            <span className="settings-muted">{t("settings.backendWaiting")}</span>
          )}
        </dd>

        <dt>{t("settings.aboutAuthor")}</dt>
        <dd>{AUTHOR_NAME}</dd>

        <dt>{t("settings.aboutContact")}</dt>
        <dd className="settings-about-contact">
          <span className="settings-about-email">{AUTHOR_EMAIL}</span>
          <Button variant="secondary" size="sm" onClick={() => open(`mailto:${AUTHOR_EMAIL}`).catch((err) => console.warn("opening the mail client failed", err))}>
            <Icon name="send" size={14} />
            {t("settings.aboutWrite")}
          </Button>
          <IconButton icon="copy" size="sm" title={t("settings.aboutCopyEmail")} onClick={copyEmail} />
        </dd>

        <dt>{t("settings.aboutWebsite")}</dt>
        <dd>
          <button type="button" className="settings-about-link" onClick={() => open(WEBSITE_URL).catch((err) => console.warn("opening the website failed", err))}>
            vibessh.dev
          </button>
        </dd>
      </dl>
    </Card>
  );
}

/**
 * The Vibe AI assistant's provider settings.
 *
 * Same write-only shape as the backup destination below: the key is
 * submitted, never returned, and a blank field means "leave the stored one
 * alone" - the form has no way to display it because the backend has no way
 * to hand it over.
 *
 * The model is a free-text field rather than a dropdown on purpose. An
 * OpenAI-compatible endpoint serves whatever its operator decided to serve -
 * OpenRouter alone offers hundreds, a self-hosted llama.cpp offers one with
 * whatever name its owner gave it - so any list shipped here would be wrong
 * within a month and would make the endpoints this feature exists to support
 * look unsupported.
 */
function AiCard() {
  const { t } = useTranslation();
  const [config, setConfig] = useState<AiConfigView | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [enabled, setEnabled] = useState(false);
  // Matches the backend's own default, so a fresh install opens on the
  // included model rather than on the form that asks for somebody
  // else's endpoint and key.
  const [provider, setProvider] = useState<AiProviderKind>("vibeSshHosted");
  const [baseUrl, setBaseUrl] = useState("");
  const [model, setModel] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [quota, setQuota] = useState<AiQuota | null>(null);
  const [usageOpen, setUsageOpen] = useState(false);
  // Why there is no allowance to show. "Not signed in" and "this backend
  // offers no included model" are different problems with different fixes,
  // and collapsing them into one message told a signed-in user to sign in.
  const [quotaReason, setQuotaReason] = useState<"signIn" | "unavailable" | null>(null);

  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [testError, setTestError] = useState<string | null>(null);
  const [testOk, setTestOk] = useState(false);

  useEffect(() => {
    getAiConfig()
      .then((loaded) => {
        setConfig(loaded);
        setEnabled(loaded.enabled);
        setProvider(loaded.provider);
        setBaseUrl(loaded.baseUrl);
        setModel(loaded.model);
      })
      .catch((err) => setLoadError(errorMessage(err, t)))
      .finally(() => setLoading(false));
    // Best-effort: not being signed in, or a backend that offers no
    // included model, both resolve to "no allowance to show" rather than
    // an error over a settings form the user may not even be here for.
    getAiQuota()
      .then((loaded) => {
        setQuota(loaded);
        setQuotaReason(null);
      })
      .catch((err) => {
        setQuota(null);
        setQuotaReason(err instanceof CommandError && err.code === "unauthorized" ? "signIn" : "unavailable");
      });
  }, [t]);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setSaving(true);
    setSaveError(null);
    setTestOk(false);
    setTestError(null);
    try {
      const saved = await setAiConfig({ enabled, provider, baseUrl, model, apiKey });
      getAiQuota()
        .then((loaded) => {
          setQuota(loaded);
          setQuotaReason(null);
        })
        .catch((err) => {
          setQuota(null);
          setQuotaReason(err instanceof CommandError && err.code === "unauthorized" ? "signIn" : "unavailable");
        });
      setConfig(saved);
      // Cleared as soon as it has been handed over, so the only copy is the
      // one in the OS keyring - not also sitting in React state for the rest
      // of the session.
      setApiKey("");
      toastSuccess(t("settings.aiSavedToast"));
    } catch (err) {
      setSaveError(errorMessage(err, t));
    } finally {
      setSaving(false);
    }
  }

  async function handleTest() {
    setTesting(true);
    setTestError(null);
    setTestOk(false);
    try {
      await testAiConnection();
      setTestOk(true);
    } catch (err) {
      setTestError(errorMessage(err, t));
    } finally {
      setTesting(false);
    }
  }

  return (
    <Card title={t("settings.aiTitle")} subtitle={t("settings.aiSubtitle")}>
      {loading ? (
        <SkeletonRows />
      ) : (
        <form className="server-form" onSubmit={handleSubmit}>
          {loadError && <p className="form-note form-note-danger form-note-spaced">{loadError}</p>}
          {saveError && <p className="form-note form-note-danger form-note-spaced">{saveError}</p>}
          <Switch checked={enabled} onChange={setEnabled} label={t("settings.aiEnable")} />
          {enabled && (
            <>
              <label className="form-field">
                <span className="form-label">{t("settings.aiProvider")}</span>
                <Select
                  value={provider}
                  onChange={(value) => setProvider(value as AiProviderKind)}
                  items={[
                    { value: "vibeSshHosted", label: t("settings.aiProviderHosted") },
                    { value: "openAiCompatible", label: t("settings.aiProviderOpenAiCompatible") },
                  ]}
                />
                <span className="form-note">
                  {provider === "vibeSshHosted" ? t("settings.aiProviderHostedNote") : t("settings.aiProviderNote")}
                </span>
              </label>
              {provider === "vibeSshHosted" && (
                <div className="settings-preference-row">
                  <div>
                    <p className="settings-preference-label">{t("aiUsage.title")}</p>
                    <p className="settings-muted">
                      {quota
                        ? t("settings.aiQuotaRemaining", { remaining: Math.max(quota.limit - quota.used, 0), limit: quota.limit })
                        : quotaReason === "signIn"
                          ? t("settings.aiQuotaSignIn")
                          : t("settings.aiQuotaUnavailable")}
                    </p>
                  </div>
                  {quota && (
                    <Button type="button" variant="secondary" size="sm" onClick={() => setUsageOpen(true)}>
                      <Icon name="activity" size={14} />
                      {t("aiUsage.open")}
                    </Button>
                  )}
                </div>
              )}
              {provider === "openAiCompatible" && (
                <>
              <label className="form-field">
                <span className="form-label">{t("settings.aiBaseUrl")}</span>
                <input
                  className="form-input"
                  value={baseUrl}
                  onChange={(e) => setBaseUrl(e.target.value)}
                  placeholder="https://openrouter.ai/api/v1"
                  autoComplete="off"
                />
                <span className="form-note">{t("settings.aiBaseUrlNote")}</span>
              </label>
              <label className="form-field">
                <span className="form-label">{t("settings.aiModel")}</span>
                <input
                  className="form-input"
                  value={model}
                  onChange={(e) => setModel(e.target.value)}
                  placeholder="meta-llama/llama-3.3-70b-instruct"
                  autoComplete="off"
                />
                <span className="form-note">{t("settings.aiModelNote")}</span>
              </label>
              <label className="form-field">
                <span className="form-label">{t("settings.aiApiKey")}</span>
                <input
                  className="form-input"
                  type="password"
                  value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)}
                  placeholder={config?.hasApiKey ? t("settings.aiApiKeyPlaceholderExisting") : t("settings.aiApiKeyPlaceholder")}
                  autoComplete="off"
                />
                <span className="form-note">{t("settings.aiApiKeyNote")}</span>
              </label>
                </>
              )}
              <Details>
                <p className="form-note">{t("settings.aiPrivacyNote")}</p>
              </Details>
            </>
          )}
          <div className="form-actions form-actions-split">
            <div>
              {config?.enabled && (
                <Button type="button" variant="secondary" size="sm" onClick={handleTest} disabled={testing}>
                  <Icon name="zap" size={14} />
                  {testing ? t("common.loading") : t("settings.aiTest")}
                </Button>
              )}
              {testOk && <span className="form-note form-note-success"> {t("settings.aiTestOk")}</span>}
              {testError && <span className="form-note form-note-danger"> {testError}</span>}
            </div>
            <Button type="submit" size="sm" disabled={saving}>
              {saving ? t("common.saving") : t("common.save")}
            </Button>
          </div>
        </form>
      )}
      {usageOpen && quota && <AiUsageModal quota={quota} onClose={() => setUsageOpen(false)} />}
    </Card>
  );
}

/** One global S3-compatible destination Application backups can additionally upload to, on top of the local `.vibessh-backups/` copy every backup already gets - see the Rust `models::BackupDestinationConfig`'s own doc comment. */
function BackupDestinationCard() {
  const { t } = useTranslation();
  const [config, setConfig] = useState<BackupDestinationConfig | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [enabled, setEnabled] = useState(false);
  const [endpoint, setEndpoint] = useState("");
  const [region, setRegion] = useState("");
  const [bucket, setBucket] = useState("");
  const [accessKeyId, setAccessKeyId] = useState("");
  const [pathPrefix, setPathPrefix] = useState("");
  const [pathStyle, setPathStyle] = useState(false);
  const [secretAccessKey, setSecretAccessKey] = useState("");

  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [testError, setTestError] = useState<string | null>(null);
  const [testOk, setTestOk] = useState(false);

  useEffect(() => {
    getBackupDestination()
      .then((loaded) => {
        setConfig(loaded);
        setEnabled(loaded.enabled);
        setEndpoint(loaded.endpoint);
        setRegion(loaded.region);
        setBucket(loaded.bucket);
        setAccessKeyId(loaded.accessKeyId);
        setPathPrefix(loaded.pathPrefix);
        setPathStyle(loaded.pathStyle);
      })
      .catch((err) => setLoadError(errorMessage(err, t)))
      .finally(() => setLoading(false));
  }, [t]);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setSaving(true);
    setSaveError(null);
    setTestOk(false);
    try {
      const saved = await setBackupDestination({ enabled, endpoint, region, bucket, accessKeyId, pathPrefix, pathStyle, secretAccessKey });
      setConfig(saved);
      setSecretAccessKey("");
      toastSuccess(t("settings.backupDestinationSavedToast"));
    } catch (err) {
      setSaveError(errorMessage(err, t));
    } finally {
      setSaving(false);
    }
  }

  async function handleTest() {
    setTesting(true);
    setTestError(null);
    setTestOk(false);
    try {
      await testBackupDestination();
      setTestOk(true);
    } catch (err) {
      setTestError(errorMessage(err, t));
    } finally {
      setTesting(false);
    }
  }

  return (
    <Card title={t("settings.backupDestinationTitle")} subtitle={t("settings.backupDestinationSubtitle")}>
      {loading ? (
        <SkeletonRows />
      ) : (
        <form className="server-form" onSubmit={handleSubmit}>
          {loadError && <p className="form-note form-note-danger form-note-spaced">{loadError}</p>}
          {saveError && <p className="form-note form-note-danger form-note-spaced">{saveError}</p>}
          <Switch checked={enabled} onChange={setEnabled} label={t("settings.backupDestinationEnable")} />
          {enabled && (
            <>
              <label className="form-field">
                <span className="form-label">{t("settings.backupDestinationEndpoint")}</span>
                <input className="form-input" value={endpoint} onChange={(e) => setEndpoint(e.target.value)} placeholder="https://s3.amazonaws.com" />
              </label>
              <div className="form-row">
                <label className="form-field">
                  <span className="form-label">{t("settings.backupDestinationRegion")}</span>
                  <input className="form-input" value={region} onChange={(e) => setRegion(e.target.value)} placeholder="us-east-1" />
                </label>
                <label className="form-field">
                  <span className="form-label">{t("settings.backupDestinationBucket")}</span>
                  <input className="form-input" value={bucket} onChange={(e) => setBucket(e.target.value)} />
                </label>
              </div>
              <label className="form-field">
                <span className="form-label">{t("settings.backupDestinationAccessKey")}</span>
                <input className="form-input" value={accessKeyId} onChange={(e) => setAccessKeyId(e.target.value)} autoComplete="off" />
              </label>
              <label className="form-field">
                <span className="form-label">{t("settings.backupDestinationSecretKey")}</span>
                <input
                  className="form-input"
                  type="password"
                  value={secretAccessKey}
                  onChange={(e) => setSecretAccessKey(e.target.value)}
                  placeholder={config?.enabled ? t("settings.backupDestinationSecretKeyPlaceholderExisting") : t("settings.backupDestinationSecretKeyPlaceholder")}
                  autoComplete="off"
                />
              </label>
              <label className="form-field">
                <span className="form-label">{t("settings.backupDestinationPathPrefix")}</span>
                <input className="form-input" value={pathPrefix} onChange={(e) => setPathPrefix(e.target.value)} placeholder="vibessh-backups" />
              </label>
              <Checkbox checked={pathStyle} onChange={setPathStyle} label={t("settings.backupDestinationPathStyle")} />
              <p className="form-note">{t("settings.backupDestinationNote")}</p>
            </>
          )}
          <div className="form-actions form-actions-split">
            <div>
              {config?.enabled && (
                <Button type="button" variant="secondary" size="sm" onClick={handleTest} disabled={testing}>
                  <Icon name="zap" size={14} />
                  {testing ? t("common.loading") : t("settings.backupDestinationTest")}
                </Button>
              )}
              {testOk && <span className="form-note form-note-success"> {t("settings.backupDestinationTestOk")}</span>}
              {testError && <span className="form-note form-note-danger"> {testError}</span>}
            </div>
            <Button type="submit" size="sm" disabled={saving}>
              {saving ? t("common.saving") : t("common.save")}
            </Button>
          </div>
        </form>
      )}
    </Card>
  );
}

/**
 * Which backend the account and team features talk to.
 *
 * **Why this had to exist.** The address has always been a per-install
 * setting - VibeSSH ships a backend you run yourself, under `apps/backend/` - and
 * it defaults to `http://localhost:8787`, this machine's own dev instance.
 * The command to change it existed, and the service wrapper existed, and
 * nothing in the interface ever called either. So every install kept the
 * default, every attempt to register or sign in reached for a port on the
 * user's own computer, and the error read "couldn't reach the VibeSSH cloud
 * backend" with a localhost URL in it that nobody could do anything about.
 *
 * Everything else in VibeSSH - nodes, applications, files, terminals - works
 * without this. Accounts, teams and shared servers are what need it.
 */
function CloudBackendCard() {
  const { t } = useTranslation();
  const [current, setCurrent] = useState<string | null>(null);
  const [url, setUrl] = useState("");
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    cloudGetBackendUrl()
      .then((loaded) => {
        setCurrent(loaded);
        setUrl(loaded);
      })
      .catch((err) => setLoadError(errorMessage(err, t)))
      .finally(() => setLoading(false));
  }, [t]);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    // A trailing slash here becomes a double slash in every request path,
    // which some servers answer and some reject - so it is taken off once,
    // here, rather than guarded against at each call site.
    const trimmed = url.trim().replace(/\/+$/, "");
    setSaving(true);
    setSaveError(null);
    try {
      await cloudSetBackendUrl(trimmed);
      setCurrent(trimmed);
      setUrl(trimmed);
      toastSuccess(t("settings.cloudBackendSavedToast"));
    } catch (err) {
      setSaveError(errorMessage(err, t));
    } finally {
      setSaving(false);
    }
  }

  // Said plainly, and not in red.
  //
  // Somebody using VibeSSH on their own has nothing wrong with their
  // install: nodes, applications, files and the terminal all work with no
  // account at all. Painting this card as an error told them something was
  // broken when the honest word is "off". The warning belongs where somebody
  // actually tries to sign in - see `AuthModal`.
  const [unconfigured, setUnconfigured] = useState(false);
  useEffect(() => {
    cloudBackendIsConfigured()
      .then((yes) => setUnconfigured(!yes))
      .catch(() => undefined);
  }, [current]);

  return (
    <Card title={t("settings.cloudBackendTitle")} subtitle={t("settings.cloudBackendSubtitle")}>
      {/* Straight to the page that explains running one, because "point this
          at your own backend" is not advice somebody can act on without it. */}
      <p className="form-note settings-guide-link">
        <GuideLink topic="cloud-backend" />
        <span>{t("settings.cloudBackendGuide")}</span>
      </p>
      {loading ? (
        <SkeletonRows />
      ) : (
        <form className="server-form" onSubmit={handleSubmit}>
          {loadError && <p className="form-note form-note-danger form-note-spaced">{loadError}</p>}
          {saveError && <p className="form-note form-note-danger form-note-spaced">{saveError}</p>}
          {unconfigured && <p className="form-note form-note-spaced">{t("settings.cloudBackendUnset")}</p>}
          <label className="form-field">
            <span className="form-label">{t("settings.cloudBackendLabel")}</span>
            <input className="form-input" value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://vibessh.example.com" />
          </label>
          <Details>
            <p className="form-note">{t("settings.cloudBackendNote")}</p>
          </Details>
          <div className="form-actions">
            <Button type="submit" size="sm" disabled={saving || url.trim() === "" || url.trim() === current}>
              {saving ? t("common.saving") : t("common.save")}
            </Button>
          </div>
        </form>
      )}
    </Card>
  );
}

/** The suffix every Vibe Network / Private DNS alias gets (e.g. `.vibe`) - configurable per install. Only affects aliases created from now on, so changing it never breaks an already-working alias. */
function DnsSuffixCard() {
  const { t } = useTranslation();
  const [current, setCurrent] = useState<string | null>(null);
  const [suffix, setSuffixValue] = useState("");
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    getDnsSuffix()
      .then((loaded) => {
        setCurrent(loaded);
        setSuffixValue(loaded);
      })
      .catch((err) => setLoadError(errorMessage(err, t)))
      .finally(() => setLoading(false));
  }, [t]);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setSaving(true);
    setSaveError(null);
    try {
      const saved = await setDnsSuffix(suffix);
      setCurrent(saved);
      setSuffixValue(saved);
      toastSuccess(t("settings.dnsSuffixSavedToast"));
    } catch (err) {
      setSaveError(errorMessage(err, t));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Card title={t("settings.dnsSuffixTitle")} subtitle={t("settings.dnsSuffixSubtitle")}>
      {loading ? (
        <SkeletonRows />
      ) : (
        <form className="server-form" onSubmit={handleSubmit}>
          {loadError && <p className="form-note form-note-danger form-note-spaced">{loadError}</p>}
          {saveError && <p className="form-note form-note-danger form-note-spaced">{saveError}</p>}
          <label className="form-field">
            <span className="form-label">{t("settings.dnsSuffixLabel")}</span>
            <input className="form-input" value={suffix} onChange={(e) => setSuffixValue(e.target.value)} placeholder=".vibe" />
          </label>
          <p className="form-note">{t("settings.dnsSuffixNote")}</p>
          <div className="form-actions form-actions-split">
            <div>{current && suffix !== current && <span className="form-note">{t("settings.dnsSuffixCurrent", { suffix: current })}</span>}</div>
            <Button type="submit" size="sm" disabled={saving || suffix === current}>
              {saving ? t("common.saving") : t("common.save")}
            </Button>
          </div>
        </form>
      )}
    </Card>
  );
}

/** Login for a private Docker registry (Docker Hub, ghcr.io, a self-hosted one) - one row per registry host, reused by every Application whose image comes from it. `docker login` happens on the Node itself right before a pull that needs it; nothing here touches any Application directly. */
function RegistryCredentialsCard() {
  const { t } = useTranslation();
  const [credentials, setCredentials] = useState<RegistryCredential[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  function load() {
    setLoading(true);
    setLoadError(null);
    listRegistryCredentials()
      .then(setCredentials)
      .catch((err) => setLoadError(errorMessage(err, t)))
      .finally(() => setLoading(false));
  }

  useEffect(load, []);

  async function handleDelete(id: string) {
    setDeletingId(id);
    setActionError(null);
    try {
      await removeRegistryCredential(id);
      load();
    } catch (err) {
      setActionError(errorMessage(err, t));
    } finally {
      setDeletingId(null);
    }
  }

  return (
    <Card title={t("settings.registryTitle")} subtitle={t("settings.registrySubtitle")}>
      {loadError && <p className="form-note form-note-danger form-note-spaced">{loadError}</p>}
      {actionError && <p className="form-note form-note-danger form-note-spaced">{actionError}</p>}
      <div className="application-detail-header-row">
        <p className="form-note">{t("settings.registryNote")}</p>
        <Button size="sm" onClick={() => setAddOpen(true)}>
          <Icon name="plus" size={14} />
          {t("settings.registryAdd")}
        </Button>
      </div>

      {loading ? (
        <SkeletonRows />
      ) : credentials.length === 0 ? (
        <EmptyState icon="key" title={t("settings.registryEmptyTitle")} description={t("settings.registryEmptyDescription")} />
      ) : (
        <ul className="server-list">
          {credentials.map((credential) => (
            <li key={credential.id} className="server-list-item">
              <div className="server-list-main">
                <span className="server-list-name">{credential.registry}</span>
                <span className="server-list-host">{credential.username}</span>
              </div>
              <IconButton
                icon="trash"
                size="sm"
                danger
                title={t("settings.registryDeleteAria")}
                onClick={() => handleDelete(credential.id)}
                disabled={deletingId === credential.id}
              />
            </li>
          ))}
        </ul>
      )}

      {addOpen && (
        <AddRegistryCredentialModal
          onClose={() => setAddOpen(false)}
          onAdded={() => {
            setAddOpen(false);
            load();
          }}
        />
      )}
    </Card>
  );
}

interface AddRegistryCredentialModalProps {
  onClose: () => void;
  onAdded: () => void;
}

function AddRegistryCredentialModal({ onClose, onAdded }: AddRegistryCredentialModalProps) {
  const { t } = useTranslation();
  const backdrop = useModalDialog(onClose, { labelledBy: "settings-dialog-title-1" });
  const [registry, setRegistry] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!registry.trim() || !username.trim() || !password) {
      setError(t("settings.registryInvalidForm"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await setRegistryCredential({ registry: registry.trim(), username: username.trim(), password });
      onAdded();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" {...backdrop.backdropProps}>
      <div className="modal-panel modal-panel-sm" {...backdrop.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="settings-dialog-title-1">{t("settings.registryAddTitle")}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <form className="server-form" onSubmit={handleSubmit}>
          <div className="modal-body">
            {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
            <label className="form-field">
              <span className="form-label">{t("settings.registryHost")}</span>
              <input className="form-input" value={registry} onChange={(e) => setRegistry(e.target.value)} placeholder="docker.io" autoFocus />
              <p className="form-note">{t("settings.registryHostHelp")}</p>
            </label>
            <label className="form-field">
              <span className="form-label">{t("settings.registryUsername")}</span>
              <input className="form-input" value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="off" />
            </label>
            <label className="form-field">
              <span className="form-label">{t("settings.registryPassword")}</span>
              <input className="form-input" type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="off" />
            </label>
            <div className="form-actions">
              <Button type="button" variant="secondary" onClick={onClose} disabled={busy}>
                {t("common.cancel")}
              </Button>
              <Button type="submit" disabled={busy}>
                {busy ? t("common.saving") : t("common.save")}
              </Button>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}

/**
 * Whether the close button quits VibeSSH or leaves it beside the clock.
 *
 * Its own component so that failing to read the setting costs this one row
 * rather than the whole Settings page - and so the switch is not rendered at
 * all until the real value is known, which is the difference between "off"
 * and "not loaded yet". A switch that shows the wrong position for a moment
 * is a switch somebody will click twice.
 */
function MinimizeToTrayRow() {
  const { t } = useTranslation();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getTraySettings()
      .then((settings) => setEnabled(settings.minimizeToTray))
      .catch((err) => setError(errorMessage(err, t)));
  }, [t]);

  async function change(next: boolean) {
    const previous = enabled;
    setEnabled(next);
    setError(null);
    try {
      await setMinimizeToTray(next);
    } catch (err) {
      // Put back, because the switch is a claim about what will happen when
      // the window is closed, and a claim that did not reach disk is false.
      setEnabled(previous);
      setError(errorMessage(err, t));
    }
  }

  return (
    <div className="settings-preference-row settings-preference-row-stacked">
      <div>
        <p className="settings-preference-label">{t("settings.minimizeToTray")}</p>
        <p className="settings-muted">{t("settings.minimizeToTrayDescription")}</p>
        {error && <p className="form-note form-note-danger">{error}</p>}
      </div>
      {enabled !== null && <Switch checked={enabled} onChange={(next) => void change(next)} label={t("settings.minimizeToTrayLabel")} />}
    </div>
  );
}
