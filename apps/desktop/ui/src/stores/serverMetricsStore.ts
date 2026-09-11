import { create } from "zustand";
import { getServerMetrics } from "@/services/monitorService";
import type { ServerMetrics } from "@/types/serverEvent";

/**
 * The last metrics reading for each server, shared between the screens that
 * ask for it.
 *
 * The Dashboard and the Monitor page each polled their own copy, which was
 * fine while they were the only readers. The rail's hover card is not: it
 * appears for a second, and opening an SSH connection every time a pointer
 * crosses an icon would be a round trip per hover for a number that changes
 * slowly.
 *
 * So readings are cached and shared. A hover uses whatever the Dashboard
 * already fetched; if nothing has, it fetches once and everything else gets
 * it for free.
 */

/** How long a reading is reused before another hover will fetch again.
 *
 * Total memory and disk size do not change on a running machine, and the
 * used figures move slowly enough that a minute-old number is honest. This
 * is not the Monitor page - that has its own poll, and its own reason to be
 * current to the second. */
const FRESH_FOR_MS = 60_000;

interface CachedMetrics {
  metrics: ServerMetrics;
  fetchedAt: number;
}

interface ServerMetricsState {
  byServer: Record<string, CachedMetrics>;
  /** Servers with a request in flight, so two hovers do not both fetch. */
  inFlight: Record<string, true>;
  /** Stores a reading somebody else already fetched. */
  put: (serverId: string, metrics: ServerMetrics) => void;
  /** Fetches only if there is nothing fresh and nothing already asking. */
  ensure: (serverId: string) => void;
}

export const useServerMetricsStore = create<ServerMetricsState>((set, get) => ({
  byServer: {},
  inFlight: {},

  put: (serverId, metrics) =>
    set((state) => ({ byServer: { ...state.byServer, [serverId]: { metrics, fetchedAt: Date.now() } } })),

  ensure: (serverId) => {
    const { byServer, inFlight } = get();
    const cached = byServer[serverId];
    if (inFlight[serverId]) return;
    if (cached && Date.now() - cached.fetchedAt < FRESH_FOR_MS) return;

    set((state) => ({ inFlight: { ...state.inFlight, [serverId]: true } }));
    getServerMetrics(serverId)
      .then((metrics) => get().put(serverId, metrics))
      .catch(() => {
        // Unreachable, an agent-mode Node with no metrics, or a refusal.
        // The card simply shows nothing rather than a zero, which would be
        // a reading it does not have.
      })
      .finally(() =>
        set((state) => {
          const next = { ...state.inFlight };
          delete next[serverId];
          return { inFlight: next };
        }),
      );
  },
}));

/** The cached reading for one server, or undefined. */
export function useCachedServerMetrics(serverId: string): ServerMetrics | undefined {
  return useServerMetricsStore((state) => state.byServer[serverId]?.metrics);
}
