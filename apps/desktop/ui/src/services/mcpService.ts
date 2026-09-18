import { callCommand } from "./tauri";

/**
 * What the local endpoint is set to, and what a client needs to reach it.
 *
 * **The token is here on purpose, and it is the only place in this app where
 * a secret travels to the interface.** Everywhere else - SSH passwords, key
 * passphrases, database admin passwords - the value stays in the keyring and
 * the frontend only ever learns whether one exists. This one exists to be
 * copied into somebody else's configuration file, so showing it is the
 * feature. It is still not logged, not persisted here, and re-read from the
 * keyring on every call rather than held in a store.
 */
export interface McpSettings {
  enabled: boolean;
  allowChanges: boolean;
  port: number;
  /** The address to paste into an MCP client. */
  url: string;
  token: string;
}

export function getMcpSettings(): Promise<McpSettings> {
  return callCommand<McpSettings>("get_mcp_settings");
}

/**
 * Saves the setting and opens or closes the port in the same call.
 *
 * The Rust side writes the file only once the endpoint is actually
 * listening, so a port already taken by something else comes back as an
 * error with the switch still off - rather than a setting that claims to be
 * on and is not.
 */
export function setMcpSettings(input: { enabled: boolean; allowChanges: boolean; port: number }): Promise<McpSettings> {
  return callCommand<McpSettings>("set_mcp_settings", input);
}

/** A new token; every client holding the old one stops working at once. */
export function rotateMcpToken(): Promise<McpSettings> {
  return callCommand<McpSettings>("rotate_mcp_token");
}
