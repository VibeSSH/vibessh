import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { HostAddress } from "@/components/ui/HostAddress";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { OverflowMenu } from "@/components/ui/OverflowMenu";
import { StatusDot } from "@/components/ui/StatusDot";
import { DeleteServerDialog } from "@/components/servers/DeleteServerDialog";
import { NodeSetupWizard } from "@/components/servers/NodeSetupWizard";
import { LocalMachineCard } from "@/components/servers/LocalMachineCard";
import { NodeIcon } from "@/components/servers/NodeIcon";
import { NodeSyncBadge } from "@/components/servers/ServerCard";
import { usePingStore } from "@/stores/pingStore";
import { useServerPinging } from "@/hooks/useServerPinging";
import { deleteServer, listServers, serverSummaryToManagedServer } from "@/services/serverService";
import { useServerModalStore } from "@/stores/serverModalStore";
import { useServersStore, type ManagedServer } from "@/stores/serversStore";
import { toastSuccess } from "@/stores/toastStore";
import "./pages.css";
import "./Servers.css";
import "@/components/servers/forms.css";
// Sync-pill styles the reused NodeSyncBadge renders (server-card-sync-*).
import "@/components/servers/ServerCard.css";
import { errorMessage } from "@/services/tauri";

interface ServerRowProps {
  server: ManagedServer;
  onOpenTerminal: () => void;
  onOpenFiles: () => void;
  onOpenMonitor: () => void;
  onOpenActions: () => void;
  onOpenFirewall: () => void;
  onSetupNode: () => void;
  onEdit: () => void;
  onDelete: () => void;
}

/**
 * One Node as a compact list row (name + protocol, masked host, status and
 * latency, then actions). Replaces the old grid of large ServerCards: the
 * three most-used destinations (terminal, files, monitor) stay as inline
 * icon buttons, everything else moves into the "..." menu. Agent Nodes have
 * no SSH surfaces, so they show the sync badge in place of those buttons.
 */
function ServerRow({
  server,
  onOpenTerminal,
  onOpenFiles,
  onOpenMonitor,
  onOpenActions,
  onOpenFirewall,
  onSetupNode,
  onEdit,
  onDelete,
}: ServerRowProps) {
  const { t } = useTranslation();
  const isAgent = server.connectionMode === "agent";
  const latencyMs = usePingStore((s) => s.latencies[server.id]);
  const menuItems = isAgent
    ? [
        { label: t("common.edit"), icon: "edit", onClick: onEdit },
        { label: t("common.remove"), icon: "trash", danger: true, onClick: onDelete },
      ]
    : [
        { label: t("nav.actions"), icon: "zap", onClick: onOpenActions },
        { label: t("nav.firewall"), icon: "shield", onClick: onOpenFirewall },
        { label: t("serverCard.setupNode"), icon: "settings", onClick: onSetupNode },
        { label: t("common.edit"), icon: "edit", onClick: onEdit },
        { label: t("common.remove"), icon: "trash", danger: true, onClick: onDelete },
      ];

  return (
    <li className="server-list-item">
      <div className="server-list-icon">
        <NodeIcon server={server} size={16} />
      </div>
      <div className="server-list-main">
        <span className="server-list-name" title={server.name}>
          {server.name}
          <span className="server-row-proto">{isAgent ? "AGENT" : "SSH"}</span>
        </span>
        <HostAddress value={server.host} prefix={server.username ? `${server.username}@` : undefined} className="server-list-host" />
      </div>
      <span className="server-row-status">
        {!isAgent && server.status === "online" && typeof latencyMs === "number" && <span className="server-row-latency">{latencyMs} ms</span>}
        <StatusDot status={server.status} withLabel />
      </span>
      <div className="server-list-actions">
        {isAgent ? (
          <NodeSyncBadge serverId={server.id} name={server.name} />
        ) : (
          <>
            <IconButton icon="terminal" size="sm" title={t("serverCard.terminalAria", { name: server.name })} onClick={onOpenTerminal} />
            <IconButton icon="folder" size="sm" title={t("serverCard.browseFilesAria", { name: server.name })} onClick={onOpenFiles} />
            <IconButton icon="activity" size="sm" title={t("serverCard.monitorAria", { name: server.name })} onClick={onOpenMonitor} />
          </>
        )}
        <OverflowMenu ariaLabel={server.name} items={menuItems} />
      </div>
    </li>
  );
}

export function Servers() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [deletingServer, setDeletingServer] = useState<ManagedServer | null>(null);
  const [settingUpServer, setSettingUpServer] = useState<ManagedServer | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const servers = useServersStore((s) => s.servers);
  const setServers = useServersStore((s) => s.setServers);
  const removeServer = useServersStore((s) => s.removeServer);
  const openForCreate = useServerModalStore((s) => s.openForCreate);
  const openForEdit = useServerModalStore((s) => s.openForEdit);

  const needle = filter.trim().toLowerCase();
  const filteredServers = useMemo(
    () => (needle ? servers.filter((s) => s.name.toLowerCase().includes(needle) || s.host.toLowerCase().includes(needle)) : servers),
    [servers, needle],
  );

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

  async function handleConfirmDelete() {
    if (!deletingServer) return;
    setDeleteBusy(true);
    setDeleteError(null);
    try {
      await deleteServer(deletingServer.id);
      removeServer(deletingServer.id);
      toastSuccess(t("servers.removedToast", { name: deletingServer.name }));
      setDeletingServer(null);
    } catch (err) {
      setDeleteError(errorMessage(err, t));
    } finally {
      setDeleteBusy(false);
    }
  }

  return (
    <div className="page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">{t("servers.title")}</h1>
          <p className="page-subtitle">{t("servers.subtitle")}</p>
        </div>
        <Button onClick={openForCreate}>
          <Icon name="plug" size={16} />
          {t("servers.addServer")}
        </Button>
      </div>

      {/* Above the node list, and shown whether or not there are any nodes:
          "run it here" is a real answer to "where can this run", and it used
          to be reachable only by leaving a field blank in the wizard. */}
      <LocalMachineCard />

      {servers.length === 0 ? (
        <Card>
          <EmptyState icon="plug" title={t("servers.emptyTitle")} description={t("servers.emptyDescription")} />
        </Card>
      ) : (
        <>
          <div className="servers-filter-row">
            <Icon name="search" size={14} />
            <input
              className="form-input servers-filter-input"
              placeholder={t("servers.filterPlaceholder")}
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
          </div>

          {filteredServers.length === 0 ? (
            <Card>
              <EmptyState icon="search" title={t("servers.noMatchesTitle")} description={t("servers.noMatchesDescription", { query: filter })} />
            </Card>
          ) : (
            <Card>
              <ul className="server-list">
                {filteredServers.map((server) => (
                  <ServerRow
                    key={server.id}
                    server={server}
                    onOpenTerminal={() => navigate(`/terminal/${server.id}`)}
                    onOpenFiles={() => navigate(`/files/${server.id}`)}
                    onOpenMonitor={() => navigate(`/monitor/${server.id}`)}
                    onOpenActions={() => navigate(`/actions/${server.id}`)}
                    onOpenFirewall={() => navigate(`/firewall/${server.id}`)}
                    onSetupNode={() => setSettingUpServer(server)}
                    onEdit={() => openForEdit(server)}
                    onDelete={() => {
                      setDeleteError(null);
                      setDeletingServer(server);
                    }}
                  />
                ))}
              </ul>
            </Card>
          )}
        </>
      )}

      {deletingServer && (
        <DeleteServerDialog
          serverId={deletingServer.id}
          serverName={deletingServer.name}
          busy={deleteBusy}
          error={deleteError}
          onConfirm={handleConfirmDelete}
          onCancel={() => setDeletingServer(null)}
        />
      )}

      {settingUpServer && (
        <NodeSetupWizard serverId={settingUpServer.id} serverName={settingUpServer.name} onClose={() => setSettingUpServer(null)} />
      )}
    </div>
  );
}
