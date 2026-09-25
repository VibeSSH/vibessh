import { create } from "zustand";

/**
 * Why the prompt is open, because the three cases need different words and
 * one of them needs different controls.
 *
 * - `missing`: there is nowhere this machine could have kept the password.
 * - `rejected`: the Node refused the password it was given - type it again.
 * - `rejectedKey`: the Node refused the key. Nothing typed here can fix
 *   that, so the prompt offers the server's settings instead of a field.
 */
export type PromptReason = "missing" | "rejected" | "rejectedKey";

export interface PromptRequest {
  reason: PromptReason;
  /** The account the Node refused, when it refused one. */
  username?: string;
}

interface PendingPrompt extends PromptRequest {
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
  request: (serverId: string, request?: PromptRequest) => Promise<string | null>;
  submit: (password: string) => void;
  cancel: () => void;
}

const waiting = new Map<string, Array<(password: string | null) => void>>();

/**
 * Servers whose rejected-login prompt was dismissed.
 *
 * The dashboard and the monitor poll every few seconds, and each poll
 * against a Node with a wrong password fails the same way. Without this, a
 * "Cancel" would be answered by the same dialog on the next poll - which is
 * the modal spam that makes people stop reading modals. Cleared when the
 * server is edited, since that is where the credentials get fixed.
 */
const dismissed = new Set<string>();

export function isRejectedPromptDismissed(serverId: string): boolean {
  return dismissed.has(serverId);
}

export function clearRejectedPromptDismissal(serverId: string): void {
  dismissed.delete(serverId);
}

export const useSessionPasswordStore = create<SessionPasswordState>((set, get) => ({
  pending: null,

  request(serverId, request = { reason: "missing" }) {
    return new Promise<string | null>((resolve) => {
      const queue = waiting.get(serverId);
      if (queue) {
        queue.push(resolve);
        return;
      }
      waiting.set(serverId, [resolve]);
      set({ pending: { serverId, resolve, ...request } });
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
    if (pending.reason !== "missing") dismissed.add(pending.serverId);
    settle(pending.serverId, null);
    set({ pending: null });
  },
}));

function settle(serverId: string, password: string | null) {
  const queue = waiting.get(serverId) ?? [];
  waiting.delete(serverId);
  for (const resolve of queue) resolve(password);
}
