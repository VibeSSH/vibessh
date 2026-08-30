import type { ServerConnectionStatus } from "@/types/server";

/** Shared between ServerCard and Rail's instance list - both render the same status dot. */
export const STATUS_COLOR: Record<ServerConnectionStatus, string> = {
  online: "var(--t-status-connected)",
  offline: "var(--t-text-dim)",
  connecting: "var(--t-status-connecting)",
  unknown: "var(--t-text-dim)",
};
