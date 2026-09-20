import { useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { startMetricsStream, stopMetricsStream } from "@/services/monitorService";
import { useServerMetricsStore } from "@/stores/serverMetricsStore";
import type { ServerMetrics } from "@/types/serverEvent";
import type { ServerMetricsState } from "@/hooks/useServerMetricsPolling";

/** Enough points for a small trend line without the array growing forever. */
const HISTORY_LENGTH = 20;

/**
 * How long a stop waits before it takes effect. Switching pages unmounts and
 * remounts the Dashboard, and stopping a stream the instant the old view leaves
 * would race the new view's start: the backend's start is idempotent, so it
 * sees the not-yet-stopped stream and no-ops, and the trailing stop then kills
 * it - leaving the tiles blank until the next remount. Holding the stop briefly
 * lets a quick remount cancel it, so the stream simply stays alive across the
 * switch. A real departure (or a window left hidden) still stops, just a beat
 * later.
 */
const STOP_GRACE_MS = 2500;

/** Deferred stops, shared across mounts so a remount can cancel one. */
const pendingStops = new Map<string, ReturnType<typeof setTimeout>>();

function ensureStreaming(id: string) {
  const pending = pendingStops.get(id);
  if (pending !== undefined) {
    clearTimeout(pending);
    pendingStops.delete(id);
  }
  void startMetricsStream(id).catch(() => {});
}

function scheduleStop(id: string) {
  if (pendingStops.has(id)) return;
  const timer = setTimeout(() => {
    pendingStops.delete(id);
    void stopMetricsStream(id).catch(() => {});
  }, STOP_GRACE_MS);
  pendingStops.set(id, timer);
}

/**
 * Live server metrics, pushed rather than polled. For each SSH-mode server it
 * asks the backend to stream `metrics://<id>` events (one persistent sampler on
 * the server's existing SSH session) and updates as each reading arrives, so
 * the tiles move in near real time instead of on a several-second timer.
 *
 * Returns the same shape as `useServerMetricsPolling` so callers are unchanged,
 * and it shares every reading into `serverMetricsStore` for the rail's hover
 * card, exactly as the poll did.
 */
export function useServerMetricsStream(sshServerIds: string[]): Record<string, ServerMetricsState> {
  const [state, setState] = useState<Record<string, ServerMetricsState>>({});
  const key = sshServerIds.join(",");

  useEffect(() => {
    const ids = key ? key.split(",") : [];
    if (ids.length === 0) return;

    // Seed from the shared cache so a remount shows the last reading at once,
    // rather than a blank tile until the next event ~1.5s later.
    setState((prev) => {
      const seeded = { ...prev };
      for (const id of ids) {
        const cached = useServerMetricsStore.getState().byServer[id]?.metrics;
        if (cached && !seeded[id]) seeded[id] = { latest: cached, history: [cached] };
      }
      return seeded;
    });

    const unlisteners: UnlistenFn[] = [];
    let disposed = false;

    // The listeners stay registered for the effect's whole life; only the
    // backend sampling is toggled with visibility, so no reading is dropped
    // while the window is up.
    ids.forEach((id) => {
      void listen<ServerMetrics>(`metrics://${id}`, (event) => {
        const metrics = event.payload;
        useServerMetricsStore.getState().put(id, metrics);
        setState((prev) => ({
          ...prev,
          [id]: { latest: metrics, history: [...(prev[id]?.history ?? []), metrics].slice(-HISTORY_LENGTH) },
        }));
      }).then((un) => {
        if (disposed) un();
        else unlisteners.push(un);
      });
    });

    const startAll = () => ids.forEach(ensureStreaming);
    const stopAll = () => ids.forEach(scheduleStop);

    // Pause the SSH sampling while the window is hidden - the same saving the
    // old poll made - and resume on return.
    const onVisibility = () => {
      if (document.hidden) stopAll();
      else startAll();
    };
    if (!document.hidden) startAll();
    document.addEventListener("visibilitychange", onVisibility);

    return () => {
      disposed = true;
      document.removeEventListener("visibilitychange", onVisibility);
      unlisteners.forEach((un) => un());
      stopAll();
    };
  }, [key]);

  return state;
}
