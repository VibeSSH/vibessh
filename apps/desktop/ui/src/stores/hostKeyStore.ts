import { create } from "zustand";

/**
 * The "this Node's SSH host key changed" question, asked once per server.
 *
 * Same shape as `sessionPasswordStore`: any command anywhere can hit the
 * mismatch, `callCommand` asks here, and one dialog (`HostKeyPrompt`) answers.
 * Callers that hit it while the dialog is open wait for the same answer
 * rather than stacking dialogs.
 */
export interface HostKeyRequest {
  host: string;
  /** The fingerprint VibeSSH recorded before, when it is known. */
  expected: string | null;
  /** The fingerprint the Node presented this time - what trusting would record. */
  presented: string;
}

interface PendingHostKey extends HostKeyRequest {
  serverId: string;
}

interface HostKeyState {
  pending: PendingHostKey | null;
  /** Resolves true when the person trusts the new key, false when they do not. */
  request: (serverId: string, request: HostKeyRequest) => Promise<boolean>;
  answer: (trusted: boolean) => void;
}

const waiting = new Map<string, Array<(trusted: boolean) => void>>();

/**
 * Servers whose new key the person declined this run. Every poll on the page
 * would otherwise bring the dialog straight back; the error still shows where
 * it happened, and editing the server (or restarting VibeSSH) asks again.
 */
const declined = new Set<string>();

export function isHostKeyDeclined(serverId: string): boolean {
  return declined.has(serverId);
}

export const useHostKeyStore = create<HostKeyState>((set, get) => ({
  pending: null,

  request(serverId, request) {
    return new Promise<boolean>((resolve) => {
      const queue = waiting.get(serverId);
      if (queue) {
        queue.push(resolve);
        return;
      }
      waiting.set(serverId, [resolve]);
      set({ pending: { serverId, ...request } });
    });
  },

  answer(trusted) {
    const pending = get().pending;
    if (!pending) return;
    if (!trusted) declined.add(pending.serverId);
    const queue = waiting.get(pending.serverId) ?? [];
    waiting.delete(pending.serverId);
    set({ pending: null });
    for (const resolve of queue) resolve(trusted);
  },
}));
