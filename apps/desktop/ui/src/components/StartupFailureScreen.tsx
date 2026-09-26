import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-shell";
import { Button } from "@/components/ui/Button";
import { Icon } from "@/components/ui/Icon";
import { revealCrashReport, type StartupFailure } from "@/services/appService";
import { CommandError, errorMessage, normalizeError } from "@/services/tauri";
import "./StartupFailureScreen.css";

const DISCORD_URL = "https://discord.gg/CKAZWRJjJC";
const DOWNLOAD_URL = "https://vibessh.dev/download";
const EMAIL = "kryspekxd@gmail.com";

function openExternal(url: string) {
  open(url).catch((err) => console.warn(`opening ${url} failed`, err));
}

/**
 * What the window shows instead of the app when VibeSSH could not start - see
 * the Rust side's `crash_report`. Nothing else in the interface can work at
 * that point (none of its state was set up), so this is rendered on its own,
 * before the router and every store.
 *
 * Its job is to get the report to us: the error in words, the report file one
 * click away, and where to send it. When the cause has a known fix - a
 * database written by a newer VibeSSH - the fix comes first.
 */
export function StartupFailureScreen({ failure }: { failure: StartupFailure }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const error = normalizeError(failure.error);
  const newerDatabase = error instanceof CommandError && error.code === "database_from_newer_version";

  async function copyReport() {
    try {
      await navigator.clipboard.writeText(failure.report);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.warn("copying the report failed", err);
    }
  }

  function close() {
    getCurrentWindow()
      .close()
      .catch((err) => console.warn("closing the window failed", err));
  }

  return (
    <div className="startup-failure">
      <header className="startup-failure-titlebar" data-tauri-drag-region>
        <img src="/vibessh-mark.png" alt="" className="startup-failure-mark" data-tauri-drag-region />
        <span data-tauri-drag-region>VibeSSH</span>
        <button type="button" className="startup-failure-close" onClick={close} aria-label={t("startupFailure.close")}>
          <Icon name="x" size={16} />
        </button>
      </header>

      <main className="startup-failure-body">
        <section className="startup-failure-card">
          <div className="startup-failure-icon">
            <Icon name="alert-triangle" size={20} />
          </div>
          <h1 className="startup-failure-title">{newerDatabase ? t("startupFailure.newerTitle") : t("startupFailure.title")}</h1>
          <p className="startup-failure-lead">{newerDatabase ? t("startupFailure.newerLead") : t("startupFailure.lead")}</p>

          {newerDatabase && (
            <div className="startup-failure-fix">
              <Button onClick={() => openExternal(DOWNLOAD_URL)}>
                <Icon name="download" size={15} />
                {t("startupFailure.download")}
              </Button>
            </div>
          )}

          <div className="startup-failure-error">
            <span className="startup-failure-label">{t("startupFailure.whatHappened")}</span>
            <p>{errorMessage(error, t)}</p>
          </div>

          <div className="startup-failure-report">
            <p className="startup-failure-report-lead">{t("startupFailure.reportLead", { email: EMAIL })}</p>
            <div className="startup-failure-actions">
              <Button variant="secondary" size="sm" onClick={() => void copyReport()}>
                <Icon name={copied ? "check" : "copy"} size={14} />
                {copied ? t("startupFailure.copied") : t("startupFailure.copyReport")}
              </Button>
              {failure.reportPath && (
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => revealCrashReport().catch((err) => console.warn("opening the report's folder failed", err))}
                >
                  <Icon name="folder" size={14} />
                  {t("startupFailure.showReport")}
                </Button>
              )}
              <Button variant="secondary" size="sm" onClick={() => openExternal(DISCORD_URL)}>
                <Icon name="external-link" size={14} />
                Discord
              </Button>
            </div>
          </div>
        </section>

        <Button variant="ghost" onClick={close}>
          {t("startupFailure.close")}
        </Button>
      </main>
    </div>
  );
}
