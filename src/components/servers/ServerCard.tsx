import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { HostAddress } from "@/components/ui/HostAddress";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { usePingStore } from "@/stores/pingStore";
import { getNodeSyncStatus, reconcileAgentNode, type NodeSyncStatus } from "@/services/serverService";
import type { ManagedServer } from "@/stores/serversStore";
import "./ServerCard.css";
import { errorMessage } from "@/services/tauri";
import { StatusDot } from "@/components/ui/StatusDot";

/**
 * Etap M3 - only meaningful for an Agent-mode Node (SSH-mode has no
 * desired/applied revisioning concept yet, see
 * `services::node_state_service::reconcile_node`'s own doc comment).
 * Loads the current status on mount and after every reconcile click,
 * rather than polling - this is a manual "check in on this Node" action,
 * not a live dashboard.
 */
function NodeSyncBadge({ serverId, name }: { serverId: string; name: string }) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<NodeSyncStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  function reload() {
    getNodeSyncStatus(serverId)
      .then(setStatus)
      .catch(() => {
        // No live/persisted state yet (never reconciled this Node, or
        // running outside a real Tauri webview) - showing nothing is the
        // honest state, not an error banner on every server card.
      });
  }

  useEffect(reload, [serverId]);

  async function handleReconcile(e: React.MouseEvent) {
    e.stopPropagation();
    setBusy(true);
    setError(null);
    try {
      const outcome = await reconcileAgentNode(serverId);
      if (outcome.status === "failed") {
        setError(outcome.error ?? t("serverCard.reconcileFailed"));
      }
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
      reload();
    }
  }

  return (
    <span className="server-card-sync-group">
      {status && (
        <span
          className={`server-card-sync-pill ${status.inSync ? "server-card-sync-pill-ok" : "server-card-sync-pill-stale"}`}
          title={error ?? undefined}
        >
          {status.inSync ? t("serverCard.syncOk") : t("serverCard.syncStale")}
        </span>
      )}
      <IconButton icon="refresh-cw" size="sm" title={t("serverCard.reconcileAria", { name })} onClick={handleReconcile} disabled={busy} />
    </span>
  );
}

interface ServerCardActionButtonProps {
  icon: string;
  title: string;
  onClick: () => void;
  danger?: boolean;
}

/** Always visible on the grid card (matches Voltius's own HostCard, which passes reveal={false} for this exact layout - hover-reveal is list-mode-only there). Thin wrapper around IconButton just for the stopPropagation - clicking an action shouldn't also trigger whatever the card itself does. */
function ServerCardActionButton({ icon, title, onClick, danger }: ServerCardActionButtonProps) {
  return (
    <IconButton
      icon={icon}
      size="sm"
      danger={danger}
      title={title}
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
    />
  );
}

interface ServerCardProps {
  server: ManagedServer;
  onOpenTerminal: () => void;
  onOpenFiles: () => void;
  onOpenMonitor: () => void;
  onOpenActions: () => void;
  /** Navigates to the Firewall page for this Node (SSH-mode only - see `pages/Firewall.tsx`'s own doc comment). Omitted the same way `onEdit`/`onDelete` are on read-mostly surfaces. */
  onOpenFirewall?: () => void;
  /** Re-opens the Node Setup flow (SSH-mode only - see `NodeSetupWizard`'s own doc comment) - not just a first-run step, re-runnable any time to check/finish requirements. Omitted the same way `onEdit`/`onDelete` are on read-mostly surfaces. */
  onSetupNode?: () => void;
  /** Omitted on read-mostly surfaces (Dashboard's recent-servers view) - matches Voltius using a simpler card there instead of the full HostCard's edit/delete affordances. */
  onEdit?: () => void;
  onDelete?: () => void;
}

/**
 * Ported from Voltius's grid-layout HostCard (voltius/src/components/hosts/
 * HostCard.tsx) - the glass surface, avatar + status dot, protocol pill, and
 * especially the terminal-preview corner button (traffic-light dots +
 * user@host + blinking cursor, bleeding into the card's own corner radius)
 * are their exact visual anatomy. Dropped everything tied to features we
 * don't have (pin, team presence, cloud sync, vault move/copy, snippets) -
 * this only wires the actions VibeSSH actually has.
 */
export function ServerCard({ server, onOpenTerminal, onOpenFiles, onOpenMonitor, onOpenActions, onOpenFirewall, onSetupNode, onEdit, onDelete }: ServerCardProps) {
  const { t } = useTranslation();
  const isAgent = server.connectionMode === "agent";
  const protocolLabel = isAgent ? "AGENT" : "SSH";
  const latencyMs = usePingStore((s) => s.latencies[server.id]);

  return (
    <div className="server-card surface-glass">
      <div className="server-card-body">
        <div className="server-card-header">
          <div className={`server-card-avatar glossy-tile ${isAgent ? "server-card-avatar-agent" : ""}`}>
            <Icon name={isAgent ? "zap" : "server"} size={16} />
          </div>
          <div className="server-card-title-col">
            <div className="server-card-title-row">
              <p className="server-card-name" title={server.name}>{server.name}</p>
              <span className="server-card-protocol-pill">{protocolLabel}</span>
              <span className="server-card-status-group">
                {!isAgent && server.status === "online" && typeof latencyMs === "number" && (
                  <span className="server-card-latency">{latencyMs} ms</span>
                )}
                <StatusDot status={server.status} withLabel />
              </span>
            </div>
            <HostAddress value={server.host} prefix={server.username ? `${server.username}@` : undefined} className="server-card-host" />
          </div>
        </div>

        <div className="server-card-footer">
          <div className="server-card-actions-col">
            <div className="server-card-actions">
              {onDelete && <ServerCardActionButton icon="trash" title={t("serverCard.removeAria", { name: server.name })} danger onClick={onDelete} />}
              {onEdit && <ServerCardActionButton icon="edit" title={t("serverCard.editAria", { name: server.name })} onClick={onEdit} />}
              {!isAgent && <ServerCardActionButton icon="folder" title={t("serverCard.browseFilesAria", { name: server.name })} onClick={onOpenFiles} />}
              {!isAgent && <ServerCardActionButton icon="activity" title={t("serverCard.monitorAria", { name: server.name })} onClick={onOpenMonitor} />}
              {!isAgent && <ServerCardActionButton icon="zap" title={t("serverCard.actionsAria", { name: server.name })} onClick={onOpenActions} />}
              {!isAgent && onOpenFirewall && (
                <ServerCardActionButton icon="shield" title={t("serverCard.openFirewallAria", { name: server.name })} onClick={onOpenFirewall} />
              )}
              {isAgent && <NodeSyncBadge serverId={server.id} name={server.name} />}
            </div>
            {!isAgent && onSetupNode && (
              <div className="server-card-actions">
                <ServerCardActionButton icon="settings" title={t("serverCard.setupNodeAria", { name: server.name })} onClick={onSetupNode} />
              </div>
            )}
          </div>

          {!isAgent && (
            <button className="server-card-terminal-btn" onClick={onOpenTerminal} title={t("serverCard.terminalAria", { name: server.name })}>
              <div className="server-card-terminal-dots">
                <span className="server-card-terminal-dot" style={{ background: "#ff5f56" }} />
                <span className="server-card-terminal-dot" style={{ background: "#ffbd2e" }} />
                <span className="server-card-terminal-dot" style={{ background: "#27c93f" }} />
              </div>
              <div className="server-card-terminal-line">
                <span className="server-card-terminal-user">{server.username ?? "root"}</span>
                <span>@</span>
                <HostAddress value={server.host} className="server-card-terminal-host" interactive={false} />
                <span>
                  {" "}
                  &gt;<span className="server-card-cursor">_</span>
                </span>
              </div>
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
