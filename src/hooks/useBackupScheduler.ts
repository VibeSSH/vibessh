import { useEffect } from "react";
import i18n from "@/i18n";
import { runDueApplicationBackups } from "@/services/applicationBackupService";
import { toastSuccess } from "@/stores/toastStore";

const CHECK_INTERVAL_MS = 15 * 60 * 1000;

/**
 * Mounted once at the app shell (`AppLayout`), not per-screen - a scheduled
 * backup only ever actually runs while this timer is ticking, i.e. while
 * VibeSSH is open (see `services::application_backup_service`'s own doc
 * comment for why there's no backend equivalent). Checking every 15 minutes
 * is plenty for schedules measured in hours; the backend's own "is this
 * Application actually due" comparison is what decides anything, this is
 * just the trigger.
 */
export function useBackupScheduler() {
  useEffect(() => {
    function check() {
      runDueApplicationBackups()
        .then((created) => {
          if (created > 0) toastSuccess(i18n.t("applicationBackups.scheduledToast", { count: created }));
        })
        .catch(() => {
          // Best-effort - a transient failure here (no applications yet, a
          // Node briefly unreachable) just means the next tick tries again,
          // never worth interrupting the user over.
        });
    }
    check();
    const intervalId = window.setInterval(check, CHECK_INTERVAL_MS);
    return () => window.clearInterval(intervalId);
  }, []);
}
