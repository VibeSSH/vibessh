import { create } from "zustand";
import type { ManagedServer } from "./serversStore";

/**
 * What the add/edit server dialog is doing right now, reported by whichever
 * step is on screen - the form, agent pairing, the setup wizard.
 *
 * `busy` is what decides whether closing the dialog throws it away or hides
 * it into the background: a connection or an install in flight must not be
 * abandoned by a click on the close button, and must not leave somebody
 * guessing whether it went through.
 */
export interface ServerModalActivity {
  busy: boolean;
  /** The background task's label, e.g. "Connecting to prod-1". */
  label: string;
  /** Set when the last operation failed, so a hidden dialog can say so. */
  error: string | null;
}

const IDLE: ServerModalActivity = { busy: false, label: "", error: null };

/**
 * Global so the Rail's quick-add button can open AddServerModal from any
 * page, not just the Servers page - AppLayout renders the modal once at the
 * layout level, controlled by this store, rather than each page owning its
 * own local open/editing state.
 */
interface ServerModalState {
  isOpen: boolean;
  editingServer: ManagedServer | null;
  /** Open but hidden - its work carries on, reachable from the top bar's background tasks. */
  minimized: boolean;
  activity: ServerModalActivity;
  openForCreate: () => void;
  openForEdit: (server: ManagedServer) => void;
  close: () => void;
  minimize: () => void;
  restore: () => void;
  reportActivity: (activity: Partial<ServerModalActivity>) => void;
}

export const useServerModalStore = create<ServerModalState>((set) => ({
  isOpen: false,
  editingServer: null,
  minimized: false,
  activity: IDLE,
  // There is one of this dialog. If it is working in the background, opening
  // "another" would replace it and drop that work on the floor - so it comes
  // back instead.
  openForCreate: () =>
    set((state) => (state.isOpen && state.activity.busy ? { minimized: false } : { isOpen: true, editingServer: null, minimized: false, activity: IDLE })),
  openForEdit: (server) =>
    set((state) => (state.isOpen && state.activity.busy ? { minimized: false } : { isOpen: true, editingServer: server, minimized: false, activity: IDLE })),
  close: () => set({ isOpen: false, editingServer: null, minimized: false, activity: IDLE }),
  minimize: () => set({ minimized: true }),
  restore: () => set({ minimized: false }),
  reportActivity: (activity) => set((state) => ({ activity: { ...state.activity, ...activity } })),
}));
