import { useCallback } from "react";
import i18n from "@/i18n";
import { POLL_INTERVALS, usePolling } from "@/hooks/usePolling";
import { cloudSessionInfo, syncAdministeredTeamAccess, syncSharedApplications } from "@/services/cloudService";
import { listServers, serverSummaryToManagedServer } from "@/services/serverService";
import { useServersStore } from "@/stores/serversStore";
import { toastError } from "@/stores/toastStore";

/** Problems already shown this run, so a Node that stays unreachable is said once, not every few minutes. */
const shown = new Set<string>();

/**
 * Keeps teams working without anyone pressing Sync - mounted once at the app
 * shell, like `useBackupScheduler`.
 *
 * Both halves, in order. As an administrator of a team's Node, this install
 * writes the team's access to it whenever the team changed (the Rust side
 * decides whether anything did, so a quiet tick costs requests, not SSH).
 * As a member, it puts the team's Nodes and the applications shared with
 * this account on this install. Together that is what makes adding somebody
 * to a team enough: their app shows the applications without a button, as
 * soon as the owner's app has been open for one tick.
 */
export function useTeamSync() {
  const tick = useCallback(async () => {
    const session = await cloudSessionInfo().catch(() => null);
    if (!session?.user) return;

    try {
      for (const problem of await syncAdministeredTeamAccess()) {
        const message = problem.error
          ? i18n.t("teamSync.nodeFailed", { name: problem.serverName, message: problem.error })
          : i18n.t("teamSync.membersFailed", { name: problem.serverName, members: problem.members.join(", ") });
        if (shown.has(message)) continue;
        shown.add(message);
        toastError(message);
      }
    } catch (err) {
      console.warn("couldn't sync team access", err);
    }

    try {
      const report = await syncSharedApplications();
      if (report.serversAdded > 0 || report.serversRemoved > 0) {
        const loaded = await listServers();
        useServersStore.getState().setServers(loaded.map(serverSummaryToManagedServer));
      }
    } catch (err) {
      console.warn("couldn't sync shared applications", err);
    }
  }, []);

  usePolling(tick, POLL_INTERVALS.teamSync, { pauseWhenHidden: false });
}
