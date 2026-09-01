import { useCallback } from "react";
import i18n from "@/i18n";
import { POLL_INTERVALS, usePolling } from "@/hooks/usePolling";
import { runDueApplicationBackups } from "@/services/applicationBackupService";
import { toastSuccess } from "@/stores/toastStore";

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
  const check = useCallback(async () => {
    try {
      const created = await runDueApplicationBackups();
      if (created > 0) toastSuccess(i18n.t("applicationBackups.scheduledToast", { count: created }));
    } catch {
      // Best-effort - a transient failure here (no applications yet, a Node
      // briefly unreachable) just means the next tick tries again, never
      // worth interrupting the user over.
    }
  }, []);

  // The one poller that keeps running while the window is hidden. Every
  // other one exists to update something on screen and is pointless when
  // nothing is on screen; a scheduled backup has to happen whether or not
  // anyone is looking, which is the whole point of scheduling it.
  usePolling(check, POLL_INTERVALS.backupScheduler, { pauseWhenHidden: false });
}
