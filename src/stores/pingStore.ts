import { create } from "zustand";

interface PingState {
  /** ms, or null if the last ping failed. Absent key = never pinged yet. */
  latencies: Record<string, number | null>;
  setLatency: (serverId: string, latencyMs: number | null) => void;
}

export const usePingStore = create<PingState>((set) => ({
  latencies: {},
  setLatency: (serverId, latencyMs) =>
    set((state) => ({ latencies: { ...state.latencies, [serverId]: latencyMs } })),
}));
