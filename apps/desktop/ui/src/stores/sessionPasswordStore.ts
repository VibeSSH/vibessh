import { create } from "zustand";

interface PendingPrompt {
  serverId: string;
  /** Resolved with the typed password, or null if the person cancelled. */
  resolve: (password: string | null) => void;
}

interface SessionPasswordState {
  pending: PendingPrompt | null;
  /**
   * Asks for a password and waits.
   *
   * One prompt at a time on purpose. A single click can start several
   * commands against the same Node - a listing, a status poll, a metric -
   * and each would otherwise raise its own dialog for the same password.
   * The others wait for this one and get its answer.
   */
  request: (serverId: string) => Promise<string | null>;
  submit: (password: string) => void;
  cancel: () => void;
}

const waiting = new Map<string, Array<(password: string | null) => void>>();

export const useSessionPasswordStore = create<SessionPasswordState>((set, get) => ({
  pending: null,

  request(serverId) {
    return new Promise<string | null>((resolve) => {
      const queue = waiting.get(serverId);
      if (queue) {
        queue.push(resolve);
        return;
      }
      waiting.set(serverId, [resolve]);
      set({ pending: { serverId, resolve } });
    });
  },

  submit(password) {
    const pending = get().pending;
    if (!pending) return;
    settle(pending.serverId, password);
    set({ pending: null });
  },

  cancel() {
    const pending = get().pending;
    if (!pending) return;
    settle(pending.serverId, null);
    set({ pending: null });
  },
}));

function settle(serverId: string, password: string | null) {
  const queue = waiting.get(serverId) ?? [];
  waiting.delete(serverId);
  for (const resolve of queue) resolve(password);
}
