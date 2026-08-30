import { Icon } from "@/components/ui/Icon";
import type { ManagedServer } from "@/stores/serversStore";
import type { ServerConnectionStatus } from "@/types/server";
import "./ServerCard.css";

const STATUS_COLOR: Record<ServerConnectionStatus, string> = {
  online: "var(--t-status-connected)",
  offline: "var(--t-text-dim)",
  connecting: "var(--t-status-connecting)",
  unknown: "var(--t-text-dim)",
};

interface ServerCardActionButtonProps {
  icon: string;
  title: string;
  onClick: () => void;
  danger?: boolean;
}

/** Ported from Voltius's CardActionButton (voltius/src/components/shared/CardActionButton.tsx) - hidden until the card is hovered. */
function ServerCardActionButton({ icon, title, onClick, danger }: ServerCardActionButtonProps) {
  return (
    <button
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
      className={`server-card-action ${danger ? "server-card-action-danger" : ""}`}
      title={title}
      aria-label={title}
    >
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
  onEdit: () => void;
  onDelete: () => void;
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
  const isAgent = server.connectionMode === "agent";
  const protocolLabel = isAgent ? "AGENT" : "SSH";
  const statusColor = STATUS_COLOR[server.status];
  const identity = server.username ? `${server.username}@${server.host}` : server.host;

  return (
    <div className="server-card surface-glass">
      <div className="server-card-body">
        <div className="server-card-header">
          <div className="server-card-avatar-wrap">
            <div className="server-card-avatar">
              <Icon name={isAgent ? "zap" : "server"} size={16} />
            </div>
            <span className="server-card-status-dot" style={{ background: statusColor }}>
              {server.status === "online" && <span className="server-card-status-ping" style={{ background: statusColor }} />}
            </span>
          </div>
          <div className="server-card-title-col">
            <div className="server-card-title-row">
              <p className="server-card-name">{server.name}</p>
              <span className="server-card-protocol-pill">{protocolLabel}</span>
            </div>
            <p className="server-card-host">{identity}</p>
          </div>
        </div>

        <div className="server-card-footer">
          <div className="server-card-actions">
            <ServerCardActionButton icon="trash" title={`Remove ${server.name}`} danger onClick={onDelete} />
            <ServerCardActionButton icon="edit" title={`Edit ${server.name}`} onClick={onEdit} />
            {!isAgent && <ServerCardActionButton icon="folder" title={`Browse files on ${server.name}`} onClick={onOpenFiles} />}
            {!isAgent && <ServerCardActionButton icon="activity" title={`Monitor ${server.name}`} onClick={onOpenMonitor} />}
            {!isAgent && <ServerCardActionButton icon="zap" title={`Quick actions for ${server.name}`} onClick={onOpenActions} />}
          </div>

          {!isAgent && (
            <button className="server-card-terminal-btn" onClick={onOpenTerminal} title={`Open a terminal to ${server.name}`}>
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
