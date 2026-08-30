import { create } from "zustand";
import type { AuthenticationType, ConnectionMode, ServerConnectionStatus } from "@/types/server";
import type { AgentCapabilities } from "@/types/pairing";

/**
 * Agent-paired servers (Etap H) still live here only for the session - Etap
 * 2's server storage doesn't persist connectionMode "agent" rows yet, so
 * those vanish on app restart. SSH-mode servers loaded via `setServers` are
 * the real, persisted Etap 2 records; the sshPort/username/authenticationType/
 * privateKeyPath fields exist so the Servers page can prefill an edit form
 * or call deleteServer without a second round trip per row.
 */
export interface ManagedServer {
  id: string;
  name: string;
  host: string;
  connectionMode: ConnectionMode;
  status: ServerConnectionStatus;
  agentId?: string;
  agentVersion?: string;
  /** Only known for connectionMode "agent" - set from the handshake's Etap I capabilities. */
  capabilities?: AgentCapabilities;
  sshPort?: number;
  username?: string;
  authenticationType?: AuthenticationType;
  privateKeyPath?: string;
  /** ISO timestamp - drives "newest first" ordering (Rail's instance list) that survives an app restart, unlike relying on in-memory insertion order. Absent for agent-mode rows (not yet Etap-2-persisted, see the note above). */
  createdAt?: string;
}

interface ServersState {
  servers: ManagedServer[];
  setServers: (servers: ManagedServer[]) => void;
  upsertServer: (server: ManagedServer) => void;
  updateStatus: (id: string, status: ServerConnectionStatus) => void;
  removeServer: (id: string) => void;
}

export const useServersStore = create<ServersState>((set) => ({
  servers: [],
  setServers: (servers) => set({ servers }),
  upsertServer: (server) =>
    set((state) => {
      const existing = state.servers.findIndex((s) => s.id === server.id);
      if (existing === -1) {
        return { servers: [...state.servers, server] };
      }
      const servers = [...state.servers];
      servers[existing] = { ...servers[existing], ...server };
      return { servers };
    }),
  updateStatus: (id, status) =>
    set((state) => ({
      servers: state.servers.map((s) => (s.id === id ? { ...s, status } : s)),
    })),
  removeServer: (id) =>
    set((state) => ({
      servers: state.servers.filter((s) => s.id !== id),
    })),
}));
