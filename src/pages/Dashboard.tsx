import { useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { ServerCard } from "@/components/servers/ServerCard";
import { useServerPinging } from "@/hooks/useServerPinging";
import { listServers, serverSummaryToManagedServer } from "@/services/serverService";
import { useServersStore } from "@/stores/serversStore";
import "./pages.css";

export function Dashboard() {
  const navigate = useNavigate();
  const servers = useServersStore((s) => s.servers);
  const setServers = useServersStore((s) => s.setServers);

  useServerPinging(servers);

  useEffect(() => {
    listServers()
      .then((loaded) => setServers(loaded.map(serverSummaryToManagedServer)))
      .catch(() => {
        // No persisted servers yet, or this loaded outside a Tauri webview
        // during development - an empty list is the right fallback either way.
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const online = servers.filter((s) => s.status === "online").length;
  const offline = servers.filter((s) => s.status === "offline").length;

  return (
    <div className="page">
      <div className="page-header">
        <h1 className="page-title">Dashboard</h1>
        <p className="page-subtitle">Overview of all your connected servers.</p>
      </div>

      <div className="stat-grid">
        <Card title="Servers">
          <div className="stat-value">{servers.length}</div>
        </Card>
        <Card title="Online">
          <div className="stat-value stat-success">{online}</div>
        </Card>
        <Card title="Offline">
          <div className="stat-value stat-danger">{offline}</div>
        </Card>
        <Card title="Alerts">
          <div className="stat-value">0</div>
        </Card>
      </div>

      {servers.length === 0 ? (
        <Card>
          <EmptyState
            icon="server"
            title="No servers yet"
            description="Add your first server to see its live status, resource usage, and quick actions here."
          />
        </Card>
      ) : (
        <div className="servers-grid">
          {servers.map((server) => (
            <ServerCard
              key={server.id}
              server={server}
              onOpenTerminal={() => navigate(`/terminal/${server.id}`)}
              onOpenFiles={() => navigate(`/files/${server.id}`)}
              onOpenMonitor={() => navigate(`/monitor/${server.id}`)}
              onOpenActions={() => navigate(`/actions/${server.id}`)}
            />
          ))}
        </div>
      )}
    </div>
  );
}
