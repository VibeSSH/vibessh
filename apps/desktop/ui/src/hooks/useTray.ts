import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { setTrayLanguage, TRAY_CHECK_FOR_UPDATES_EVENT } from "@/services/trayService";
import { useUpdateStore } from "@/stores/updateStore";

/**
 * Keeps the tray menu in step with the running application.
 *
 * Two jobs, both of which have to happen where the shell is rather than on
 * some page somebody might never open.
 *
 * **The language.** The tray menu is built in Rust and cannot see what
 * `i18next` resolved to, so it is told - on startup and again whenever the
 * language changes. Without the second half, switching to English in Settings
 * would leave a Polish menu beside the clock until the next restart.
 *
 * **"Check for updates..."** The tray does not check anything itself: it
 * raises an event and the interface runs the check it already knows how to
 * run, into the banner it already has. A second updater living in the tray
 * would be a second thing to keep in step with the first, and the one place
 * a signature check could be quietly forgotten.
 */
export function useTray(): void {
  const { i18n } = useTranslation();
  const language = i18n.resolvedLanguage ?? i18n.language;

  useEffect(() => {
    if (!language) return;
    // Failure is swallowed on purpose: a tray menu stuck in the previous
    // language is a blemish, and an interface that refused to start because
    // it could not rename a menu item would be a fault.
    void setTrayLanguage(language).catch(() => undefined);
  }, [language]);

  useEffect(() => {
    // `listen` resolves to the unlisten function, so the cleanup has to wait
    // for it rather than being able to return it directly.
    const pending = listen(TRAY_CHECK_FOR_UPDATES_EVENT, () => {
      void useUpdateStore.getState().checkNow();
    }).catch(() => undefined);
    return () => {
      void pending.then((unlisten) => unlisten?.());
    };
  }, []);
}
