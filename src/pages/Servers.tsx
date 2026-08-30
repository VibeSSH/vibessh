import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { DeleteServerDialog } from "@/components/servers/DeleteServerDialog";
import { ServerCard } from "@/components/servers/ServerCard";
import { useServerPinging } from "@/hooks/useServerPinging";
import { deleteServer, listServers, serverSummaryToManagedServer } from "@/services/serverService";
import { useServerModalStore } from "@/stores/serverModalStore";
import { useServersStore, type ManagedServer } from "@/stores/serversStore";
import { toastSuccess } from "@/stores/toastStore";
import "./pages.css";
import "./Servers.css";
import "@/components/servers/forms.css";

export function Servers() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [deletingServer, setDeletingServer] = useState<ManagedServer | null>(null);
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
      setDeleteError(err instanceof Error ? err.message : t("servers.couldntRemove"));
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
            <div className="servers-grid">
              {filteredServers.map((server) => (
                <ServerCard
                  key={server.id}
                  server={server}
                  onOpenTerminal={() => navigate(`/terminal/${server.id}`)}
                  onOpenFiles={() => navigate(`/files/${server.id}`)}
                  onOpenMonitor={() => navigate(`/monitor/${server.id}`)}
                  onOpenActions={() => navigate(`/actions/${server.id}`)}
                  onEdit={() => openForEdit(server)}
                  onDelete={() => {
                    setDeleteError(null);
                    setDeletingServer(server);
                  }}
                />
              ))}
            </div>
          )}
        </>
      )}

      {deletingServer && (
        <DeleteServerDialog
          serverName={deletingServer.name}
          busy={deleteBusy}
          error={deleteError}
          onConfirm={handleConfirmDelete}
          onCancel={() => setDeletingServer(null)}
        />
      )}
    </div>
  );
}
