import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Switch } from "@/components/ui/Switch";
import { getMcpSettings, rotateMcpToken, setMcpSettings, type McpSettings } from "@/services/mcpService";
import { errorMessage } from "@/services/tauri";
import { toastSuccess } from "@/stores/toastStore";

/**
 * Letting Claude ask VibeSSH about your servers.
 *
 * **Two switches, not one.** "May Claude see my servers" and "may Claude
 * restart them" are different questions, and folding them together would
 * take the safe answer off the table. The second is only offered once the
 * first is on, because permission to change something that is not listening
 * is not a state worth being able to save.
 *
 * **The token is shown, which is unusual here.** Every other secret in this
 * app stays in the keyring and the interface only learns whether one exists.
 * This one has to be copied into a client's configuration file, so showing
 * it is the whole point - but it is masked until asked for, so a screen
 * share or a screenshot of this page does not hand it out by accident. That
 * is the same reason the rotate button exists.
 */
export function McpCard() {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<McpSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [revealed, setRevealed] = useState(false);

  useEffect(() => {
    getMcpSettings()
      .then(setSettings)
      .catch((err) => setError(errorMessage(err, t)));
  }, [t]);

  async function save(next: { enabled: boolean; allowChanges: boolean; port: number }) {
    const previous = settings;
    setBusy(true);
    setError(null);
    // Shown as chosen straight away, then put back if it did not take - the
    // usual reason it does not is a port already in use, and the switch
    // flicking back is what says so before the message is read.
    setSettings(settings ? { ...settings, ...next } : null);
    try {
      setSettings(await setMcpSettings(next));
    } catch (err) {
      setSettings(previous);
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  async function rotate() {
    setBusy(true);
    setError(null);
    try {
      setSettings(await rotateMcpToken());
      setRevealed(true);
      toastSuccess(t("settings.mcpTokenRotated"));
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  async function copy(value: string, message: string) {
    await navigator.clipboard.writeText(value);
    toastSuccess(message);
  }

  return (
    <Card title={t("settings.mcpTitle")} subtitle={t("settings.mcpSubtitle")}>
      {settings === null ? (
        error && <p className="form-note form-note-danger">{error}</p>
      ) : (
        <>
          <div className="settings-preference-row settings-preference-row-stacked">
            <div>
              <p className="settings-preference-label">{t("settings.mcpEnable")}</p>
              <p className="settings-muted">{t("settings.mcpEnableNote")}</p>
            </div>
            <Switch
              checked={settings.enabled}
              onChange={(enabled) => void save({ enabled, allowChanges: enabled ? settings.allowChanges : false, port: settings.port })}
              ariaLabel={t("settings.mcpEnable")}
            />
          </div>

          {settings.enabled && (
            <>
              <label className="form-field">
                <span className="form-label">{t("settings.mcpUrl")}</span>
                <div className="settings-copy-row">
                  <input className="form-input" readOnly value={settings.url} />
                  <Button variant="secondary" size="sm" onClick={() => void copy(settings.url, t("settings.mcpUrlCopied"))}>
                    {t("common.copy")}
                  </Button>
                </div>
                <span className="form-note">{t("settings.mcpUrlNote")}</span>
              </label>

              <label className="form-field">
                <span className="form-label">{t("settings.mcpToken")}</span>
                <div className="settings-copy-row">
                  {/* Masked by default: this page gets screenshotted and
                      screen-shared, and the token is the only thing between
                      the endpoint and every other process on the machine. */}
                  <input className="form-input" readOnly type={revealed ? "text" : "password"} value={settings.token} />
                  <Button variant="secondary" size="sm" onClick={() => setRevealed((shown) => !shown)}>
                    {revealed ? t("common.hide") : t("common.show")}
                  </Button>
                  <Button variant="secondary" size="sm" onClick={() => void copy(settings.token, t("settings.mcpTokenCopied"))}>
                    {t("common.copy")}
                  </Button>
                </div>
                <span className="form-note">{t("settings.mcpTokenNote")}</span>
              </label>

              <div className="settings-preference-row settings-preference-row-stacked">
                <div>
                  <p className="settings-preference-label">{t("settings.mcpAllowChanges")}</p>
                  <p className="settings-muted">{t("settings.mcpAllowChangesNote")}</p>
                </div>
                <Switch
                  checked={settings.allowChanges}
                  onChange={(allowChanges) => void save({ enabled: settings.enabled, allowChanges, port: settings.port })}
                  ariaLabel={t("settings.mcpAllowChanges")}
                />
              </div>

              <div className="form-actions">
                <Button variant="secondary" onClick={() => void rotate()} disabled={busy}>
                  {t("settings.mcpRotateToken")}
                </Button>
              </div>
            </>
          )}

          {error && <p className="form-note form-note-danger">{error}</p>}
        </>
      )}
    </Card>
  );
}
