import { useEffect, useMemo } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { HostAddress } from "@/components/ui/HostAddress";
import { Icon } from "@/components/ui/Icon";
import { listServers, serverSummaryToManagedServer } from "@/services/serverService";
import { useServersStore } from "@/stores/serversStore";
import "./pages.css";
import "./Servers.css";
import "./Files.css";
import { StatusDot } from "@/components/ui/StatusDot";
import { NodeIcon } from "@/components/servers/NodeIcon";

interface ModulePickerProps {
  titleKey: string;
  subtitleKey: string;
  icon: string;
  /** e.g. "/terminal" - the server picked is appended as "/:id". */
  routePrefix: string;
}

/**
 * Every per-server module (Terminal, Files, Monitor, Actions) needs a
 * specific server to act on - there's no "current server" concept outside
 * of one. Sidebar's links for those modules land here instead of jumping
 * straight to a module route with no id, so picking a server is always the
 * first step rather than a dead end.
 */
export function ModulePicker({ titleKey, subtitleKey, icon, routePrefix }: ModulePickerProps) {
  const { t } = useTranslation();
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
        <h1 className="page-title">{t(titleKey)}</h1>
        <p className="page-subtitle">{t(subtitleKey)}</p>
      </div>

      {servers.length === 0 ? (
        <Card>
          <EmptyState icon={icon} title={t("modulePicker.emptyTitle")} description={t("modulePicker.emptyDescription")} />
          <div className="page-empty-action-row">
            <Button onClick={() => navigate("/servers")}>
              <Icon name="plug" size={16} />
              {t("modulePicker.goToServers")}
            </Button>
          </div>
        </Card>
      ) : (
        <Card subtitle={t("modulePicker.pickServer")}>
          <ul className="server-list">
            {servers.map((server) => (
              <li key={server.id} className="server-list-item">
                <div className="server-list-icon">
                  <NodeIcon server={server} size={16} fallback={icon} />
                </div>
                <button className="files-entry-name" title={server.name} onClick={() => navigate(`${routePrefix}/${server.id}`)}>
                  {server.name}
                </button>
                <HostAddress value={server.host} className="server-list-host" />
                <StatusDot status={server.status} withLabel />
              </li>
            ))}
          </ul>
        </Card>
      )}
    </div>
  );
}
