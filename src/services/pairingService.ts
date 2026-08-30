import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { callCommand } from "./tauri";
import type { AgentConnectionState } from "@/types/pairing";

const PAIRING_STATE_EVENT = "agent-pairing://state";

export function generatePairingCode(): Promise<string> {
  return callCommand<string>("generate_pairing_code");
}

export function getPairingCodeTtlSeconds(): Promise<number> {
  return callCommand<number>("pairing_code_ttl_seconds");
}

export function startAgentPairing(host: string, port: number, pairingCode: string): Promise<void> {
  return callCommand<void>("start_agent_pairing", { host, port, pairingCode });
}

export function cancelAgentPairing(): Promise<void> {
  return callCommand<void>("cancel_agent_pairing");
}

/**
 * Returns an unlisten function - call it on unmount/cleanup. Resolves to a
 * no-op outside a Tauri webview (e.g. this page loaded in a plain browser
 * during development) instead of throwing - `listen()` isn't wrapped by
 * `callCommand` like invoke() calls are, so it needs its own guard here.
 */
export function onAgentPairingState(handler: (state: AgentConnectionState) => void): Promise<UnlistenFn> {
  return listen<AgentConnectionState>(PAIRING_STATE_EVENT, (event) => handler(event.payload)).catch(() => {
    return () => {};
  });
}
