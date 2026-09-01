import { useEffect, useState } from "react";
import { getServerMetrics } from "@/services/monitorService";
import type { ServerMetrics } from "@/types/serverEvent";

const POLL_INTERVAL_MS = 6000;
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

  useEffect(() => {
    if (sshServerIds.length === 0) {
      setState({});
      return;
    }
    let cancelled = false;

    async function pollOnce() {
      await Promise.all(
        sshServerIds.map(async (id) => {
          try {
            const metrics = await getServerMetrics(id);
            if (cancelled) return;
            setState((prev) => ({
              ...prev,
              [id]: { latest: metrics, history: [...(prev[id]?.history ?? []), metrics].slice(-HISTORY_LENGTH) },
            }));
          } catch {
            if (cancelled) return;
            setState((prev) => ({ ...prev, [id]: { latest: null, history: prev[id]?.history ?? [] } }));
          }
        }),
      );
    }

    pollOnce();
    const intervalId = window.setInterval(pollOnce, POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(intervalId);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  return state;
}
