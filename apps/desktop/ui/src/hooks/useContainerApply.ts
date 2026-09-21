import { useCallback } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { queryKeys } from "@/services/queryKeys";
import { recreateApplication, refreshApplicationStatus } from "@/services/applicationService";
import { useApplicationApplyStore } from "@/stores/applicationApplyStore";
import { toastError } from "@/stores/toastStore";
import { errorMessage } from "@/services/tauri";
import type { ApplicationDetail } from "@/types/application";

/**
 * Applies a just-saved config change to a running Docker container, in the
 * background.
 *
 * A Docker container bakes its environment, ports, image and resource limits
 * in at `docker create` time, so a change only takes effect once the container
 * is recreated. That recreate is several SSH round trips, and it used to be
 * awaited *inside* the save - freezing the form on a generic "Saving..." for
 * the whole time and making a one-line env edit feel like a full rebuild.
 *
 * Here it runs after the change has already been persisted and shown, so the
 * form settles immediately; the recreate is tracked in `applicationApplyStore`
 * so the detail header can say "applying changes" without blocking anything,
 * and the new status is written straight into the query cache when it lands.
 *
 * A stopped application is left stopped - there is no live container to rebuild
 * and auto-starting one the user deliberately stopped would be its own
 * surprise - matching the previous inline behaviour. The freshly-probed status
 * is used rather than a possibly-stale cached one for the same reason the old
 * inline path did (a just-started app can still read "stopped" for a few
 * seconds). A failed recreate is surfaced as a toast and does not undo the
 * change, which already saved.
 */
export function useContainerApply() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();
  const start = useApplicationApplyStore((state) => state.start);
  const finish = useApplicationApplyStore((state) => state.finish);

  return useCallback(
    async (application: ApplicationDetail) => {
      if (application.runtimeType !== "docker") return;
      start(application.id);
      try {
        const status = await refreshApplicationStatus(application.id);
        if (status === "running") {
          const newStatus = await recreateApplication(application.id);
          queryClient.setQueryData<ApplicationDetail>(queryKeys.application(application.id), (old) =>
            old ? { ...old, status: newStatus } : old,
          );
        }
      } catch (err) {
        toastError(errorMessage(err, t));
      } finally {
        finish(application.id);
      }
    },
    [queryClient, t, start, finish],
  );
}
