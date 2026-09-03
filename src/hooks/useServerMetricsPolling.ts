import { useCallback, useState } from "react";
import { getServerMetrics } from "@/services/monitorService";
import { useServerMetricsStore } from "@/stores/serverMetricsStore";
import { POLL_INTERVALS, usePolling } from "@/hooks/usePolling";
import type { ServerMetrics } from "@/types/serverEvent";

/** Enough points for a small trend line without the array growing forever. */
const HISTORY_LENGTH = 20;

export interface ServerMetricsState {
  latest: ServerMetrics | null;
  /** Oldest first, same shape Monitor.tsx's own poll loop keeps - a mini
   * sparkline only needs the CPU column out of it, the Dashboard's Activity
   * tab charts RAM too, so the full sample is kept rather than just one
   * derived number. */
  history: ServerMetrics[];
}

/**
 * Same polling shape as useServerPinging, but for the CPU/RAM/uptime reading
 * each Dashboard node card (and the Activity tab's charts) show - SSH-mode
 * only, since Monitor's own getServerMetrics has no agent-mode counterpart,
 * same restriction ServerCard already applies to its own Monitor button.
 */
export function useServerMetricsPolling(sshServerIds: string[]): Record<string, ServerMetricsState> {
  const [state, setState] = useState<Record<string, ServerMetricsState>>({});
  const key = sshServerIds.join(",");

  // Keyed on the joined id list rather than the array itself, which is a new
  // reference on every render of the caller.
  const poll = useCallback(async () => {
    const ids = key ? key.split(",") : [];
    await Promise.all(
      ids.map(async (id) => {
        try {
          const metrics = await getServerMetrics(id);
          // Shared with anything else that wants a recent reading without
          // opening its own connection - the rail's hover card, mainly. A
          // dashboard left open means hovering a Node costs nothing at all.
          useServerMetricsStore.getState().put(id, metrics);
          setState((prev) => ({
            ...prev,
            [id]: { latest: metrics, history: [...(prev[id]?.history ?? []), metrics].slice(-HISTORY_LENGTH) },
          }));
        } catch {
          setState((prev) => ({ ...prev, [id]: { latest: null, history: prev[id]?.history ?? [] } }));
        }
      }),
    );
  }, [key]);

  usePolling(poll, POLL_INTERVALS.serverMetrics, { enabled: sshServerIds.length > 0 });

  return state;
}
