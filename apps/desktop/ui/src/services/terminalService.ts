import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { callCommand } from "./tauri";

export function openTerminal(serverId: string, cols: number, rows: number): Promise<string> {
  return callCommand<string>("open_terminal", { serverId, cols, rows });
}

export function writeToTerminal(terminalId: string, data: string): Promise<void> {
  return callCommand<void>("write_to_terminal", { terminalId, data });
}

export function resizeTerminal(terminalId: string, cols: number, rows: number): Promise<void> {
  return callCommand<void>("resize_terminal", { terminalId, cols, rows });
}

export function closeTerminal(terminalId: string): Promise<void> {
  return callCommand<void>("close_terminal", { terminalId });
}

/** Same not-in-a-Tauri-webview guard as pairingService's event helpers - see there for why. */
export function onTerminalOutput(terminalId: string, handler: (chunk: string) => void): Promise<UnlistenFn> {
  return listen<string>(`terminal://${terminalId}/output`, (event) => handler(event.payload)).catch(() => () => {});
}

export function onTerminalClosed(terminalId: string, handler: (reason: string | null) => void): Promise<UnlistenFn> {
  return listen<string | null>(`terminal://${terminalId}/closed`, (event) => handler(event.payload)).catch(() => () => {});
}
