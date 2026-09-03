import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { callCommand } from "./tauri";
import type { ImportOutcome, ImportProgress, NodeOverride, PterodactylConnectionView, PterodactylMigrationPlan } from "@/types/pterodactyl";

/** Whether a key is stored. Never the key itself. */
export function getPterodactylConnection(): Promise<PterodactylConnectionView> {
  return callCommand<PterodactylConnectionView>("pterodactyl_connection");
}

/**
 * Verifies the address and the key against the panel, and stores the key only
 * once the panel has accepted it. Returns how many servers the panel reports.
 */
export function connectPterodactyl(baseUrl: string, apiKey: string): Promise<number> {
  return callCommand<number>("pterodactyl_connect", { baseUrl, apiKey });
}

/** Reads the panel and returns what it would become here. Writes nothing. */
export function planPterodactylMigration(baseUrl: string, nodeOverrides: NodeOverride[] = []): Promise<PterodactylMigrationPlan> {
  return callCommand<PterodactylMigrationPlan>("pterodactyl_plan", { baseUrl, nodeOverrides });
}

/** Forgets the stored key. */
export function forgetPterodactyl(): Promise<void> {
  return callCommand<void>("pterodactyl_forget");
}

/**
 * Runs the migration for the chosen servers.
 *
 * Only the ids travel: Rust rebuilds the plan from the panel rather than
 * trusting one sent from here, so nothing in the webview can choose what an
 * imported Application is made of. See the command's own doc comment.
 */
export function importPterodactyl(
  baseUrl: string,
  sourceIds: number[],
  targetDatabaseHostId?: string,
  nodeOverrides: NodeOverride[] = [],
  targetServerId?: string,
): Promise<ImportOutcome[]> {
  return callCommand<ImportOutcome[]>("pterodactyl_import", {
    baseUrl,
    sourceIds,
    targetServerId: targetServerId ?? null,
    targetDatabaseHostId: targetDatabaseHostId ?? null,
    nodeOverrides,
  });
}

/** Per-server progress while an import runs. */
export function onPterodactylProgress(handler: (progress: ImportProgress) => void): Promise<UnlistenFn> {
  // Same not-in-a-Tauri-webview guard the other services use, so the page
  // still renders under `npm run dev` in a plain browser.
  return listen<ImportProgress>("pterodactyl://import/progress", (event) => handler(event.payload)).catch(() => () => {});
}
