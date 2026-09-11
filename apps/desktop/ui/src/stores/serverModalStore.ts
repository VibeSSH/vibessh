import { create } from "zustand";
import type { ManagedServer } from "./serversStore";

/**
 * Global so the Rail's quick-add button can open AddServerModal from any
 * page, not just the Servers page - AppLayout renders the modal once at the
 * layout level, controlled by this store, rather than each page owning its
 * own local open/editing state.
 */
interface ServerModalState {
  isOpen: boolean;
  editingServer: ManagedServer | null;
  openForCreate: () => void;
  openForEdit: (server: ManagedServer) => void;
  close: () => void;
}

export const useServerModalStore = create<ServerModalState>((set) => ({
  isOpen: false,
  editingServer: null,
  openForCreate: () => set({ isOpen: true, editingServer: null }),
  openForEdit: (server) => set({ isOpen: true, editingServer: server }),
  close: () => set({ isOpen: false, editingServer: null }),
}));
