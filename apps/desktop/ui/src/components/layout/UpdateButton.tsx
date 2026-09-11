import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Icon } from "@/components/ui/Icon";
import { useClickOutside } from "@/hooks/useClickOutside";
import { useUpdateStore } from "@/stores/updateStore";
import { formatBytes } from "@/utils/formatBytes";
import { useRef } from "react";
import "./UpdateButton.css";

/**
 * The cloud in the rail, and what it has to say.
 *
 * Green only when there is genuinely a newer version to take. The rest of the
 * time it is the same muted grey as its neighbours - a badge that is always
 * lit teaches people to stop reading it, and this one is meant to be worth
 * noticing on the day it changes.
 */
export function UpdateButton() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const panelRef = useRef<HTMLDivElement>(null);
  const { phase, version, notes, downloaded, total, error, checkNow, install, dismissError } = useUpdateStore();

  useClickOutside(panelRef, () => setOpen(false), open);

  const hasUpdate = phase === "available" || phase === "downloading" || phase === "ready";
  const percent = total && total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : null;

  return (
    <div className="rail-update" ref={panelRef}>
      <button
        className={`rail-btn rail-update-btn ${hasUpdate ? "rail-update-btn-available" : ""}`.trim()}
        onClick={() => setOpen((o) => !o)}
        aria-label={hasUpdate ? t("update.availableAria", { version }) : t("update.checkAria")}
        title={hasUpdate ? t("update.availableAria", { version }) : t("update.checkAria")}
      >
        <Icon name="cloud" size={18} />
        {/* A dot rather than a number: there is only ever one update on
            offer, so a count would always read "1". */}
        {hasUpdate && <span className="rail-update-dot" aria-hidden="true" />}
      </button>

      {open && (
        <div className="rail-update-panel" role="dialog" aria-label={t("update.title")}>
          <p className="rail-update-title">{t("update.title")}</p>

          {phase === "checking" && <p className="rail-update-note">{t("update.checking")}</p>}

          {phase === "idle" && (
            <>
              <p className="rail-update-note">{t("update.upToDate")}</p>
              <Button variant="secondary" size="sm" onClick={() => void checkNow()}>
                {t("update.checkNow")}
              </Button>
            </>
          )}

          {phase === "available" && (
            <>
              <p className="rail-update-version">{t("update.available", { version })}</p>
              {notes && <p className="rail-update-notes">{notes}</p>}
              {/* Said before the button, not after: this restarts the app, and
                  somebody watching a server deploy should get to choose the
                  moment. */}
              <p className="rail-update-note">{t("update.restartWarning")}</p>
              <Button size="sm" onClick={() => void install()}>
                {t("update.install")}
              </Button>
            </>
          )}

          {phase === "downloading" && (
            <>
              <p className="rail-update-note">
                {percent === null ? t("update.downloading") : t("update.downloadingPercent", { percent })}
              </p>
              <div className="rail-update-bar" aria-hidden="true">
                <div className="rail-update-bar-fill" style={percent === null ? undefined : { width: `${percent}%` }} />
              </div>
              <p className="rail-update-note">
                {total ? `${formatBytes(downloaded)} / ${formatBytes(total)}` : formatBytes(downloaded)}
              </p>
            </>
          )}

          {phase === "ready" && <p className="rail-update-note">{t("update.ready")}</p>}

          {phase === "error" && (
            <>
              <p className="rail-update-note rail-update-error">{error}</p>
              <Button variant="secondary" size="sm" onClick={dismissError}>
                {t("common.close")}
              </Button>
            </>
          )}
        </div>
      )}
    </div>
  );
}
