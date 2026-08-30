import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { AddServerModal } from "@/components/servers/AddServerModal";
import { DeleteServerDialog } from "@/components/servers/DeleteServerDialog";
import { ServerCard } from "@/components/servers/ServerCard";
import { deleteServer, listServers, serverSummaryToManagedServer } from "@/services/serverService";
import { useServersStore, type ManagedServer } from "@/stores/serversStore";
import { toastSuccess } from "@/stores/toastStore";
import "./pages.css";
import "./Servers.css";
import "@/components/servers/forms.css";

export function Servers() {
  const navigate = useNavigate();
  const [modalOpen, setModalOpen] = useState(false);
  const [editingServer, setEditingServer] = useState<ManagedServer | null>(null);
  const [deletingServer, setDeletingServer] = useState<ManagedServer | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const servers = useServersStore((s) => s.servers);
  const setServers = useServersStore((s) => s.setServers);
  const removeServer = useServersStore((s) => s.removeServer);

  const needle = filter.trim().toLowerCase();
  const filteredServers = useMemo(
    () => (needle ? servers.filter((s) => s.name.toLowerCase().includes(needle) || s.host.toLowerCase().includes(needle)) : servers),
    [servers, needle],
  );

  useEffect(() => {
    listServers()
      .then((loaded) => setServers(loaded.map(serverSummaryToManagedServer)))
      .catch(() => {
        // No persisted servers yet, or this loaded outside a Tauri webview
        // during development - an empty list is the right fallback either way.
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function closeModal() {
    setModalOpen(false);
    setEditingServer(null);
  }

  async function handleConfirmDelete() {
    if (!deletingServer) return;
    setDeleteBusy(true);
    setDeleteError(null);
    try {
      await deleteServer(deletingServer.id);
      removeServer(deletingServer.id);
      toastSuccess(`Removed ${deletingServer.name}`);
      setDeletingServer(null);
    } catch (err) {
      setDeleteError(err instanceof Error ? err.message : "Couldn't remove the server.");
    } finally {
      setDeleteBusy(false);
    }
  }

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
        <>
          <div className="servers-filter-row">
            <Icon name="search" size={14} />
            <input
              className="form-input servers-filter-input"
              placeholder="Filter servers..."
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
          </div>

          {filteredServers.length === 0 ? (
            <Card>
              <EmptyState icon="search" title="No matches" description={`Nothing matches "${filter}".`} />
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
                  onEdit={() => {
                    setEditingServer(server);
                    setModalOpen(true);
                  }}
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

      {modalOpen && <AddServerModal onClose={closeModal} editingServer={editingServer ?? undefined} />}
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
