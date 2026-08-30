import { useEffect, useMemo } from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { listServers, serverSummaryToManagedServer } from "@/services/serverService";
import { useServersStore } from "@/stores/serversStore";
import "./pages.css";
import "./Servers.css";
import "./Files.css";

interface ModulePickerProps {
  title: string;
  subtitle: string;
  icon: string;
  /** e.g. "/terminal" - the server picked is appended as "/:id". */
  routePrefix: string;
}

/**
 * Every per-server module (Terminal, Files, Monitor, Actions) needs a
 * specific server to act on - there's no "current server" concept outside
 * of one. Sidebar links for those modules land here instead of jumping
 * straight to a module route with no id, so picking a server is always the
 * first step rather than a dead end.
 */
export function ModulePicker({ title, subtitle, icon, routePrefix }: ModulePickerProps) {
  const navigate = useNavigate();
  const setServers = useServersStore((s) => s.setServers);
  const allServers = useServersStore((s) => s.servers);
  const servers = useMemo(() => allServers.filter((server) => server.connectionMode === "ssh"), [allServers]);

  useEffect(() => {
    listServers()
      .then((loaded) => setServers(loaded.map(serverSummaryToManagedServer)))
      .catch(() => {
        // Outside a Tauri webview, or no servers saved yet - an empty list
        // is the right fallback either way (see Servers.tsx, same pattern).
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="page">
      <div className="page-header">
        <h1 className="page-title">{title}</h1>
        <p className="page-subtitle">{subtitle}</p>
      </div>

      {servers.length === 0 ? (
        <Card>
          <EmptyState
            icon={icon}
            title="No servers yet"
            description="Add an SSH server first, then come back here to pick one."
          />
          <div style={{ display: "flex", justifyContent: "center", marginTop: 12 }}>
            <Button onClick={() => navigate("/servers")}>
              <Icon name="plug" size={16} />
              Go to Servers
            </Button>
          </div>
        </Card>
      ) : (
        <Card subtitle="Pick a server">
          <ul className="server-list">
            {servers.map((server) => (
              <li key={server.id} className="server-list-item">
                <div className="server-list-icon">
                  <Icon name={icon} size={16} />
                </div>
                <button className="files-entry-name" onClick={() => navigate(`${routePrefix}/${server.id}`)}>
                  {server.name}
                </button>
                <span className="server-list-host">{server.host}</span>
              </li>
            ))}
          </ul>
        </Card>
      )}
    </div>
  );
}
