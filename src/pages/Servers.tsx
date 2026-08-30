import { useState } from "react";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { AddServerModal } from "@/components/servers/AddServerModal";
import { CapabilityBadges } from "@/components/servers/CapabilityBadges";
import { useServersStore } from "@/stores/serversStore";
import type { ServerConnectionStatus } from "@/types/server";
import "./pages.css";
import "./Servers.css";

const STATUS_TONE: Record<ServerConnectionStatus, "success" | "danger" | "warning" | "neutral"> = {
  online: "success",
  offline: "danger",
  connecting: "warning",
  unknown: "neutral",
};

const STATUS_LABEL: Record<ServerConnectionStatus, string> = {
  online: "Online",
  offline: "Offline",
  connecting: "Connecting",
  unknown: "Unknown",
};

export function Servers() {
  const [modalOpen, setModalOpen] = useState(false);
  const servers = useServersStore((s) => s.servers);

  return (
    <div className="page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">Servers</h1>
          <p className="page-subtitle">Manage the remote servers VibeSSH connects to.</p>
        </div>
        <Button onClick={() => setModalOpen(true)}>
          <Icon name="plug" size={16} />
          Add server
        </Button>
      </div>

      {servers.length === 0 ? (
        <Card>
          <EmptyState
            icon="plug"
            title="No servers yet"
            description="Add a server via SSH, or pair a Vibe Agent for realtime metrics and a more capable terminal."
          />
        </Card>
      ) : (
        <Card>
          <ul className="server-list">
            {servers.map((server) => (
              <li key={server.id} className="server-list-item">
                <div className="server-list-icon">
                  <Icon name={server.connectionMode === "agent" ? "zap" : "terminal"} size={16} />
                </div>
                <div className="server-list-main">
                  <span className="server-list-name">{server.name}</span>
                  <span className="server-list-host">{server.host}</span>
                  {server.capabilities && <CapabilityBadges capabilities={server.capabilities} />}
                </div>
                <Badge tone="neutral">{server.connectionMode === "agent" ? "Agent" : "SSH"}</Badge>
                <Badge tone={STATUS_TONE[server.status]}>{STATUS_LABEL[server.status]}</Badge>
              </li>
            ))}
          </ul>
        </Card>
      )}

      {modalOpen && <AddServerModal onClose={() => setModalOpen(false)} />}
    </div>
  );
}
