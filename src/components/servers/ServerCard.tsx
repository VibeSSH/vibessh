import { useTranslation } from "react-i18next";
import { Icon } from "@/components/ui/Icon";
import { useRipple } from "@/hooks/useRipple";
import { usePingStore } from "@/stores/pingStore";
import type { ManagedServer } from "@/stores/serversStore";
import { STATUS_COLOR } from "@/utils/serverStatusColor";
import "./ServerCard.css";

interface ServerCardActionButtonProps {
  icon: string;
  title: string;
  onClick: () => void;
  danger?: boolean;
}

/** Ported from Voltius's CardActionButton (voltius/src/components/shared/CardActionButton.tsx) - always visible on a grid card (their own HostCard passes reveal={false} for this exact layout; hover-reveal is list-mode-only there), plus the same ripple-on-press every one of their buttons has. */
function ServerCardActionButton({ icon, title, onClick, danger }: ServerCardActionButtonProps) {
  const { createRipple, rippleEls } = useRipple();
  return (
    <button
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
      onPointerDown={createRipple}
      className={`server-card-action ripple-host ${danger ? "server-card-action-danger" : ""}`}
      title={title}
      aria-label={title}
    >
      {rippleEls}
      <Icon name={icon} size={15} />
    </button>
  );
}

interface ServerCardProps {
  server: ManagedServer;
  onOpenTerminal: () => void;
  onOpenFiles: () => void;
  onOpenMonitor: () => void;
  onOpenActions: () => void;
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
export function ServerCard({ server, onOpenTerminal, onOpenFiles, onOpenMonitor, onOpenActions, onEdit, onDelete }: ServerCardProps) {
  const { t } = useTranslation();
  const isAgent = server.connectionMode === "agent";
  const protocolLabel = isAgent ? "AGENT" : "SSH";
  const statusColor = STATUS_COLOR[server.status];
  const identity = server.username ? `${server.username}@${server.host}` : server.host;
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
              <p className="server-card-name">{server.name}</p>
              <span className="server-card-protocol-pill">{protocolLabel}</span>
              <span className="server-card-status-group">
                {!isAgent && server.status === "online" && typeof latencyMs === "number" && (
                  <span className="server-card-latency">{latencyMs} ms</span>
                )}
                <span className="server-card-status-dot-wrap">
                  {server.status === "online" && <span className="server-card-status-ping" style={{ background: statusColor }} />}
                  <span className="server-card-status-dot" style={{ background: statusColor }} />
                </span>
              </span>
            </div>
            <p className="server-card-host">{identity}</p>
          </div>
        </div>

        <div className="server-card-footer">
          <div className="server-card-actions">
            {onDelete && <ServerCardActionButton icon="trash" title={t("serverCard.removeAria", { name: server.name })} danger onClick={onDelete} />}
            {onEdit && <ServerCardActionButton icon="edit" title={t("serverCard.editAria", { name: server.name })} onClick={onEdit} />}
            {!isAgent && <ServerCardActionButton icon="folder" title={t("serverCard.browseFilesAria", { name: server.name })} onClick={onOpenFiles} />}
            {!isAgent && <ServerCardActionButton icon="activity" title={t("serverCard.monitorAria", { name: server.name })} onClick={onOpenMonitor} />}
            {!isAgent && <ServerCardActionButton icon="zap" title={t("serverCard.actionsAria", { name: server.name })} onClick={onOpenActions} />}
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
                <span className="server-card-terminal-host">{server.host}</span>
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
