import { create } from "zustand";

export type TransferDirection = "upload" | "download";
export type TransferStatus = "waiting" | "transferring" | "done" | "error" | "canceled";

export interface TransferItem {
  id: string;
  name: string;
  direction: TransferDirection;
  status: TransferStatus;
  transferred: number;
  total: number;
  /** Instantaneous bytes/sec, computed from the gap between the last two progress events - not a running average, so it settles quickly after a slow start. */
  speedBps: number;
  error?: string;
  /** Kept around after the transfer finishes so "Retry" can re-issue the exact same request. */
  retry?: () => void;
  lastEventAt?: number;
  lastEventBytes?: number;
}

interface FileTransferState {
  transfers: TransferItem[];
  addTransfer: (item: Omit<TransferItem, "status" | "transferred" | "total" | "speedBps"> & { total: number }) => void;
  updateProgress: (id: string, transferred: number, total: number) => void;
  markDone: (id: string) => void;
  markError: (id: string, error: string) => void;
  markCanceled: (id: string) => void;
  clearCompleted: () => void;
  removeTransfer: (id: string) => void;
}

export const useFileTransferStore = create<FileTransferState>((set) => ({
  transfers: [],

  addTransfer: (item) =>
    set((state) => ({
      transfers: [{ ...item, status: "transferring", transferred: 0, speedBps: 0 }, ...state.transfers],
    })),

  updateProgress: (id, transferred, total) =>
    set((state) => ({
      transfers: state.transfers.map((t) => {
        if (t.id !== id) return t;
        const now = Date.now();
        const elapsedSeconds = t.lastEventAt ? (now - t.lastEventAt) / 1000 : 0;
        const deltaBytes = t.lastEventBytes !== undefined ? transferred - t.lastEventBytes : 0;
        const speedBps = elapsedSeconds > 0 ? deltaBytes / elapsedSeconds : t.speedBps;
        return { ...t, transferred, total, speedBps, status: "transferring", lastEventAt: now, lastEventBytes: transferred };
      }),
    })),

  markDone: (id) => set((state) => ({ transfers: state.transfers.map((t) => (t.id === id ? { ...t, status: "done", speedBps: 0 } : t)) })),

  markError: (id, error) =>
    set((state) => ({ transfers: state.transfers.map((t) => (t.id === id ? { ...t, status: "error", error, speedBps: 0 } : t)) })),

  markCanceled: (id) => set((state) => ({ transfers: state.transfers.map((t) => (t.id === id ? { ...t, status: "canceled", speedBps: 0 } : t)) })),

  clearCompleted: () => set((state) => ({ transfers: state.transfers.filter((t) => t.status === "transferring" || t.status === "waiting") })),

  removeTransfer: (id) => set((state) => ({ transfers: state.transfers.filter((t) => t.id !== id) })),
}));
