import { useCallback, useEffect, useState } from "react";
import { Navigate, useNavigate, useParams } from "react-router-dom";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { listServerServices, restartServerService } from "@/services/actionsService";
import { useServersStore } from "@/stores/serversStore";
import type { ServiceSummary } from "@/types/serverEvent";
import "./pages.css";
import "./Actions.css";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";

const MAX_ROWS_SHOWN = 200;

export function ActionsPage() {
  const { serverId } = useParams<{ serverId: string }>();
  const navigate = useNavigate();
  const server = useServersStore((s) => s.servers.find((srv) => srv.id === serverId));

  const [services, setServices] = useState<ServiceSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [confirming, setConfirming] = useState<ServiceSummary | null>(null);
  const [restarting, setRestarting] = useState(false);
  const [restartError, setRestartError] = useState<string | null>(null);

  const load = useCallback(() => {
    if (!serverId) return;
    setLoading(true);
    setError(null);
    listServerServices(serverId)
      .then((loaded) => setServices([...loaded].sort((a, b) => a.name.localeCompare(b.name))))
      .catch((err) => setError(err instanceof Error ? err.message : "Couldn't list services."))
      .finally(() => setLoading(false));
  }, [serverId]);

  useEffect(load, [load]);

  if (!serverId) {
    return <Navigate to="/servers" replace />;
  }

  const needle = filter.trim().toLowerCase();
  const filtered = needle ? services.filter((s) => s.name.toLowerCase().includes(needle)) : services;

  async function handleConfirmRestart() {
    if (!confirming || !serverId) return;
    setRestarting(true);
    setRestartError(null);
    try {
      await restartServerService(serverId, confirming.name);
      setConfirming(null);
      load();
    } catch (err) {
      setRestartError(err instanceof Error ? err.message : "Couldn't restart this service.");
    } finally {
      setRestarting(false);
    }
  }

  return (
    <div className="page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">{server ? server.name : "Actions"}</h1>
          <p className="page-subtitle">{server ? server.host : serverId}</p>
        </div>
        <Button variant="secondary" onClick={() => navigate("/servers")}>
          <Icon name="chevron-left" size={16} />
          Back to servers
        </Button>
      </div>

      {error && <p className="page-error-note">{error}</p>}

      <Card title="Systemd services" subtitle={`${services.length} units`}>
        <input
          className="form-input actions-filter"
          placeholder="Filter by name..."
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        {loading ? (
          <p className="settings-muted">Loading...</p>
        ) : (
          <ul className="server-list">
            {filtered.slice(0, MAX_ROWS_SHOWN).map((service) => (
              <li key={service.name} className="server-list-item">
                <div className="server-list-main">
                  <span className="server-list-name">{service.name}</span>
                  <span className="server-list-host">{service.description}</span>
                </div>
                <Badge tone={service.active ? "success" : "neutral"}>{service.active ? "Active" : "Inactive"}</Badge>
                <Badge tone="neutral">{service.enabled ? "Enabled" : "Disabled"}</Badge>
                <button
                  className="server-list-action"
                  aria-label={`Restart ${service.name}`}
                  onClick={() => {
                    setConfirming(service);
                    setRestartError(null);
                  }}
                >
                  <Icon name="zap" size={14} />
                </button>
              </li>
            ))}
          </ul>
        )}
      </Card>

      {confirming && (
        <div className="modal-backdrop" onClick={() => !restarting && setConfirming(null)}>
          <div className="modal-panel" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2 className="modal-title">Restart service</h2>
              <button className="modal-close" onClick={() => setConfirming(null)} aria-label="Close">
                <Icon name="x" size={16} />
              </button>
            </div>
            <div className="modal-body">
              <p style={{ margin: "0 0 16px", fontSize: 13, color: "var(--text-primary)", lineHeight: 1.5 }}>
                Restart <strong>{confirming.name}</strong>? Anything using it may briefly disconnect.
              </p>
              {restartError && (
                <p className="form-note" style={{ color: "var(--danger)", marginBottom: 12 }}>
                  {restartError}
                </p>
              )}
              <div className="form-actions" style={{ gap: 8 }}>
                <Button variant="secondary" onClick={() => setConfirming(null)} disabled={restarting}>
                  Cancel
                </Button>
                <Button variant="danger" onClick={handleConfirmRestart} disabled={restarting}>
                  Restart
                </Button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
