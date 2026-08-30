import { useEffect } from "react";
import { pingServer } from "@/services/serverService";
import { usePingStore } from "@/stores/pingStore";
import { useServersStore, type ManagedServer } from "@/stores/serversStore";

const POLL_INTERVAL_MS = 15000;

/**
 * Polls every SSH-mode server in `servers` on a fixed interval and feeds the
 * result into both usePingStore (the latency reading itself) and
 * useServersStore's updateStatus (online/offline) - the status dot on
 * ServerCard reads server.status directly, so this is what makes it mean
 * something for SSH servers instead of staying permanently "unknown".
 * Agent-mode servers already get real status from their own WebSocket
 * connection and are left alone here.
 */
export function useServerPinging(servers: ManagedServer[]) {
  const setLatency = usePingStore((s) => s.setLatency);
  const updateStatus = useServersStore((s) => s.updateStatus);
  const sshServerIds = servers.filter((s) => s.connectionMode === "ssh").map((s) => s.id);
  const key = sshServerIds.join(",");

  useEffect(() => {
    if (sshServerIds.length === 0) return;
    let cancelled = false;

    async function pollOnce() {
      await Promise.all(
        sshServerIds.map(async (id) => {
          try {
            const latencyMs = await pingServer(id);
            if (cancelled) return;
            setLatency(id, latencyMs);
            updateStatus(id, "online");
          } catch {
            if (cancelled) return;
            setLatency(id, null);
            updateStatus(id, "offline");
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
}
