import { useCallback } from "react";
import { pingServer } from "@/services/serverService";
import { POLL_INTERVALS, usePolling } from "@/hooks/usePolling";
import { usePingStore } from "@/stores/pingStore";
import { useServersStore, type ManagedServer } from "@/stores/serversStore";

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

  // Keyed on the joined id list rather than the array itself, which is a
  // new reference on every render of the caller.
  const poll = useCallback(async () => {
    const ids = key ? key.split(",") : [];
    await Promise.all(
      ids.map(async (id) => {
        try {
          const latencyMs = await pingServer(id);
          setLatency(id, latencyMs);
          updateStatus(id, "online");
        } catch {
          setLatency(id, null);
          updateStatus(id, "offline");
        }
      }),
    );
  }, [key, setLatency, updateStatus]);

  usePolling(poll, POLL_INTERVALS.serverPing, { enabled: sshServerIds.length > 0 });
}
