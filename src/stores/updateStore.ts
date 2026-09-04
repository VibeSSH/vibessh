import { create } from "zustand";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

/** How often the app looks for a new release while it is running. */
const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

export type UpdatePhase = "idle" | "checking" | "available" | "downloading" | "ready" | "error";

interface UpdateState {
  phase: UpdatePhase;
  /** The version on offer, once one is known. */
  version: string | null;
  notes: string | null;
  /** Bytes in, and the total when the server declared one. */
  downloaded: number;
  total: number | null;
  error: string | null;
  /** Held between the check and the install - `downloadAndInstall` lives on it. */
  pending: Update | null;
  checkNow: () => Promise<void>;
  install: () => Promise<void>;
  dismissError: () => void;
}

/**
 * Whether a newer VibeSSH exists, and pulling it down.
 *
 * Built on Tauri's own updater rather than a hand-rolled download. That
 * plugin verifies a **signature** on the installer before running it, against
 * a public key compiled into the app. Without that, an update mechanism is a
 * remote-code-execution feature with extra steps: anything that could answer
 * the update URL - a hijacked host, a proxy, a poisoned DNS answer - could
 * hand the user an installer of its own choosing and it would be run with
 * their consent.
 *
 * Nothing here installs on its own. The app says an update exists; the person
 * decides when their servers can afford the restart.
 */
export const useUpdateStore = create<UpdateState>((set, get) => ({
  phase: "idle",
  version: null,
  notes: null,
  downloaded: 0,
  total: null,
  error: null,
  pending: null,

  async checkNow() {
    if (get().phase === "downloading") return;
    set({ phase: "checking", error: null });
    try {
      const update = await check();
      if (update) {
        set({ phase: "available", version: update.version, notes: update.body ?? null, pending: update });
      } else {
        set({ phase: "idle", version: null, notes: null, pending: null });
      }
    } catch (err) {
      // A failed check is not worth interrupting anybody over - it usually
      // means no network. It is recorded so the panel can say why, and the
      // next scheduled check simply tries again.
      set({ phase: "error", error: err instanceof Error ? err.message : String(err) });
    }
  },

  async install() {
    const update = get().pending;
    if (!update) return;
    set({ phase: "downloading", downloaded: 0, total: null, error: null });
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") set({ total: event.data.contentLength ?? null });
        else if (event.event === "Progress") set({ downloaded: get().downloaded + event.data.chunkLength });
        else if (event.event === "Finished") set({ phase: "ready" });
      });
      // On Windows the installer takes over and closes the app itself; the
      // relaunch is what covers the platforms where it does not.
      await relaunch();
    } catch (err) {
      set({ phase: "error", error: err instanceof Error ? err.message : String(err) });
    }
  },

  dismissError() {
    set({ phase: "idle", error: null });
  },
}));

/**
 * Starts the periodic check.
 *
 * Called once from the layout. The first check is deferred a few seconds so
 * it never competes with the first paint or with the connections the app
 * opens on startup.
 */
export function startUpdateChecks(): () => void {
  const first = window.setTimeout(() => void useUpdateStore.getState().checkNow(), 5_000);
  const interval = window.setInterval(() => void useUpdateStore.getState().checkNow(), CHECK_INTERVAL_MS);
  return () => {
    window.clearTimeout(first);
    window.clearInterval(interval);
  };
}
