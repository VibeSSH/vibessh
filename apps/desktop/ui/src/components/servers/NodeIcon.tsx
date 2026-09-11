import { Icon } from "@/components/ui/Icon";
import type { ManagedServer } from "@/stores/serversStore";
import "./NodeIcon.css";

interface NodeIconProps {
  /** The node whose icon to show. `undefined` renders the fallback glyph -
   * a Database Host with no linked node, say. */
  server?: Pick<ManagedServer, "icon" | "connectionMode">;
  size?: number;
  /** The glyph to draw when this node has no icon of its own. Defaults to
   * the connection-mode glyph the rest of the app uses. */
  fallback?: string;
}

/**
 * A node's custom icon, or the glyph that stands in for it.
 *
 * Written once because the same six lines had been copied into the rail, the
 * server card, the dashboard's node cards and the module picker, and were
 * about to be copied into the Vibe Network and Database Hosts pages too. That
 * is exactly the shape the audit kept finding - `shell_quote` in thirteen
 * modules, the connect-retry block in four - where each copy is fine until
 * one of them needs to change.
 */
export function NodeIcon({ server, size = 16, fallback }: NodeIconProps) {
  if (server?.icon) {
    return <img src={server.icon} alt="" className="node-icon-image" width={size} height={size} />;
  }
  return <Icon name={fallback ?? (server?.connectionMode === "agent" ? "zap" : "server")} size={size} />;
}
