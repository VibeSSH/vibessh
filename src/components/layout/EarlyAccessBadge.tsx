import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import "./EarlyAccessBadge.css";

/** Injected by vite from `package.json` - see `vite.config.ts`. Used only as
 * the fallback below. */
declare const __APP_VERSION__: string;

/**
 * "Early Access" plus the build's version, next to the wordmark.
 *
 * The version comes from Tauri's own `getVersion()`, which reads
 * `tauri.conf.json` - the number that actually ships - rather than from
 * `package.json`. The two are kept in step by hand today, and a badge that
 * quietly disagrees with the installer is worse than no badge. The
 * vite-injected constant is the fallback for running the frontend outside a
 * Tauri webview, where `getVersion` does not exist; the import is dynamic for
 * the same reason `TitleBar` resolves its window lazily, since a static one
 * throws on load in a plain browser.
 *
 * Deliberately not dismissible. It says the software is pre-release, which
 * stays true until it isn't, and an operator pointing this at their servers
 * should be able to see that at a glance rather than remember it.
 */
export function EarlyAccessBadge() {
  const { t } = useTranslation();
  const [version, setVersion] = useState<string>(typeof __APP_VERSION__ === "string" ? __APP_VERSION__ : "");

  useEffect(() => {
    let cancelled = false;
    void import("@tauri-apps/api/app")
      .then(({ getVersion }) => getVersion())
      .then((live) => {
        if (!cancelled) setVersion(live);
      })
      .catch(() => {
        // Not in a Tauri webview. The injected version already showed.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <span className="early-access-badge" title={t("earlyAccess.tooltip")}>
      <span className="early-access-badge-label">{t("earlyAccess.label")}</span>
      {version && <span className="early-access-badge-version">{version}</span>}
    </span>
  );
}
