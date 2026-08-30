import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { AddServerModal } from "@/components/servers/AddServerModal";
import { CapabilityBadges } from "@/components/servers/CapabilityBadges";
import { DeleteServerDialog } from "@/components/servers/DeleteServerDialog";
import { deleteServer, listServers, serverSummaryToManagedServer } from "@/services/serverService";
import { useServersStore, type ManagedServer } from "@/stores/serversStore";
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
  const navigate = useNavigate();
  const [modalOpen, setModalOpen] = useState(false);
  const [editingServer, setEditingServer] = useState<ManagedServer | null>(null);
  const [deletingServer, setDeletingServer] = useState<ManagedServer | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const servers = useServersStore((s) => s.servers);
  const setServers = useServersStore((s) => s.setServers);
  const removeServer = useServersStore((s) => s.removeServer);

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
                {server.connectionMode === "ssh" && (
                  <div className="server-list-actions">
                    <button
                      className="server-list-action"
                      aria-label={`Open a terminal to ${server.name}`}
                      onClick={() => navigate(`/terminal/${server.id}`)}
                    >
                      <Icon name="terminal" size={14} />
                    </button>
                    <button
                      className="server-list-action"
                      aria-label={`Browse files on ${server.name}`}
                      onClick={() => navigate(`/files/${server.id}`)}
                    >
                      <Icon name="folder" size={14} />
                    </button>
                    <button
                      className="server-list-action"
                      aria-label={`Monitor ${server.name}`}
                      onClick={() => navigate(`/monitor/${server.id}`)}
                    >
                      <Icon name="activity" size={14} />
                    </button>
                    <button
                      className="server-list-action"
                      aria-label={`Edit ${server.name}`}
                      onClick={() => {
                        setEditingServer(server);
                        setModalOpen(true);
                      }}
                    >
                      <Icon name="edit" size={14} />
                    </button>
                    <button
                      className="server-list-action"
                      aria-label={`Remove ${server.name}`}
                      onClick={() => {
                        setDeleteError(null);
                        setDeletingServer(server);
                      }}
                    >
                      <Icon name="trash" size={14} />
                    </button>
                  </div>
                )}
              </li>
            ))}
          </ul>
        </Card>
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
