import { create } from "zustand";
import type { ConnectionMode, ServerConnectionStatus } from "@/types/server";

/**
 * Etap 2 (SQLite server repository) isn't built yet, so this is
 * intentionally session-only - added servers vanish on app restart. It
 * exists so Etap H's UI has somewhere real to put what pairing produces
 * instead of mocking a list that goes nowhere.
 */
export interface ManagedServer {
  id: string;
  name: string;
  host: string;
  connectionMode: ConnectionMode;
  status: ServerConnectionStatus;
  agentId?: string;
  agentVersion?: string;
}

interface ServersState {
  servers: ManagedServer[];
  upsertServer: (server: ManagedServer) => void;
  updateStatus: (id: string, status: ServerConnectionStatus) => void;
  removeServer: (id: string) => void;
}

export const useServersStore = create<ServersState>((set) => ({
  servers: [],
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
