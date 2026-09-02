import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Checkbox } from "@/components/ui/Checkbox";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { Switch } from "@/components/ui/Switch";
import { useModalDialog } from "@/hooks/useModalDialog";
import { getAiConfig, setAiConfig, testAiConnection } from "@/services/aiService";
import { getAppInfo } from "@/services/appService";
import { getBackupDestination, setBackupDestination, testBackupDestination } from "@/services/applicationBackupService";
import { listRegistryCredentials, removeRegistryCredential, setRegistryCredential } from "@/services/applicationService";
import { getDnsSuffix, setDnsSuffix } from "@/services/networkService";
import { toastSuccess } from "@/stores/toastStore";
import type { AiConfigView, AiProviderKind } from "@/types/ai";
import type { BackupDestinationConfig, RegistryCredential } from "@/types/application";
import { SUPPORTED_LANGUAGES, type SupportedLanguage } from "@/i18n";
import "./pages.css";
import "./Settings.css";
import "@/components/servers/forms.css";
import "@/components/servers/AddServerModal.css";
import { errorMessage } from "@/services/tauri";

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

      <AiCard />

      <BackupDestinationCard />

      <RegistryCredentialsCard />

      <DnsSuffixCard />

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
  const [provider, setProvider] = useState<AiProviderKind>("openAiCompatible");
  const [baseUrl, setBaseUrl] = useState("");
  const [model, setModel] = useState("");
  const [apiKey, setApiKey] = useState("");

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
  }, [t]);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setSaving(true);
    setSaveError(null);
    setTestOk(false);
    setTestError(null);
    try {
      const saved = await setAiConfig({ enabled, provider, baseUrl, model, apiKey });
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
                <select className="form-input" value={provider} onChange={(e) => setProvider(e.target.value as AiProviderKind)}>
                  <option value="openAiCompatible">{t("settings.aiProviderOpenAiCompatible")}</option>
                </select>
                <span className="form-note">{t("settings.aiProviderNote")}</span>
              </label>
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
              <p className="form-note">{t("settings.aiPrivacyNote")}</p>
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
