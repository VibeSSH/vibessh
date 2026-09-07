import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { useUpdateStore } from "@/stores/updateStore";
import { formatBytes } from "@/utils/formatBytes";
import "./UpdateBanner.css";

/** Which version's banner has been waved away, so a dismissal survives a
 * restart. Per version on purpose - see `dismiss`. */
const DISMISSED_KEY = "vibessh.update.dismissed";

function readDismissed(): string | null {
  try {
    return window.localStorage.getItem(DISMISSED_KEY);
  } catch {
    // A browser with site data blocked throws on the read itself. Nothing to
    // remember is the same as nothing dismissed.
    return null;
  }
}

/**
 * Says out loud that a new version is waiting.
 *
 * **Why the cloud in the rail was not enough.** The app has always checked on
 * its own - five seconds after start and every six hours after that - and
 * announced the result by turning a small icon green and putting a dot on it.
 * People did not notice, which is how "check for updates" ended up being
 * something they clicked by hand: the information was there and nothing drew
 * the eye to it. A row across the top of the page cannot be missed in the
 * same way.
 *
 * **Dismissal is per version, not forever.** Waving this away means "not this
 * one, not now" - the next release brings it back, because the alternative is
 * a person who dismissed a banner in March and never hears about an update
 * again. Nothing here installs anything; the button is the same
 * `install()` the panel in the rail has always called, and the restart is
 * still the user's to choose.
 */
export function UpdateBanner() {
  const { t } = useTranslation();
  const { phase, version, downloaded, total, install } = useUpdateStore();
  const [dismissed, setDismissed] = useState<string | null>(readDismissed);

  // A newer version than the one that was waved away brings the banner back.
  useEffect(() => {
    if (phase === "available" && version && dismissed && dismissed !== version) {
      setDismissed(null);
    }
  }, [phase, version, dismissed]);

  function dismiss() {
    if (!version) return;
    setDismissed(version);
    try {
      window.localStorage.setItem(DISMISSED_KEY, version);
    } catch {
      // Remembering it only for this session is a smaller failure than
      // refusing to close the banner at all.
    }
  }

  const downloading = phase === "downloading";
  const ready = phase === "ready";
  // While it is downloading or done, the banner stays whatever was chosen
  // earlier: hiding the progress of something the user just started would
  // read as the click having done nothing.
  const showing = (phase === "available" && version !== dismissed) || downloading || ready;
  if (!showing || !version) return null;

  const percent = total && total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : null;

  return (
    <div className="update-banner" role="status">
      <Icon name="cloud" size={16} />
      <span className="update-banner-text">
        {ready
          ? t("update.readyBanner")
          : downloading
            ? t("update.downloadingBanner", {
                progress: percent !== null ? `${percent}%` : formatBytes(downloaded),
              })
            : t("update.availableBanner", { version })}
      </span>

      {phase === "available" && (
        <>
          {/* Said here rather than only in the rail's panel: this is now the
              place the decision is actually made. */}
          <span className="update-banner-note">{t("update.restartWarning")}</span>
          <Button size="sm" onClick={() => void install()}>
            {t("update.install")}
          </Button>
          <IconButton icon="x" size="sm" onClick={dismiss} title={t("update.dismissAria", { version })} />
        </>
      )}
    </div>
  );
}
