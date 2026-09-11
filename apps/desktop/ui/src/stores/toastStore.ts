import { create } from "zustand";

export type ToastTone = "success" | "error" | "info";

export interface Toast {
  id: string;
  message: string;
  tone: ToastTone;
}

export interface NotificationEntry extends Toast {
  at: number;
}

const AUTO_DISMISS_MS = 4000;
/** Notification history is a real log of what already happened (every toastSuccess/toastError call site in the app), capped so it can't grow unbounded over a long session. */
const HISTORY_LIMIT = 30;

interface ToastState {
  toasts: Toast[];
  history: NotificationEntry[];
  push: (message: string, tone?: ToastTone) => void;
  dismiss: (id: string) => void;
  clearHistory: () => void;
}

export const useToastStore = create<ToastState>((set) => ({
  toasts: [],
  history: [],
  push: (message, tone = "info") => {
    const id = crypto.randomUUID();
    set((state) => ({
      toasts: [...state.toasts, { id, message, tone }],
      history: [{ id, message, tone, at: Date.now() }, ...state.history].slice(0, HISTORY_LIMIT),
    }));
    window.setTimeout(() => {
      set((state) => ({ toasts: state.toasts.filter((t) => t.id !== id) }));
    }, AUTO_DISMISS_MS);
  },
  dismiss: (id) => set((state) => ({ toasts: state.toasts.filter((t) => t.id !== id) })),
  clearHistory: () => set({ history: [] }),
}));

/** Convenience wrappers - most call sites just want "say this happened", not the store's setter shape. */
export function toastSuccess(message: string) {
  useToastStore.getState().push(message, "success");
}

export function toastError(message: string) {
  useToastStore.getState().push(message, "error");
}
