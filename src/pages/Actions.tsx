import { useCallback, useEffect, useState } from "react";
import { Navigate, useNavigate, useParams } from "react-router-dom";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import {
  listServerContainers,
  listServerServices,
  restartServerContainer,
  restartServerService,
} from "@/services/actionsService";
import { useServersStore } from "@/stores/serversStore";
import { toastSuccess } from "@/stores/toastStore";
import type { ContainerSummary, ServiceSummary } from "@/types/serverEvent";
import "./pages.css";
import "./Actions.css";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";

const MAX_ROWS_SHOWN = 200;

type PendingRestart =
  | { kind: "service"; name: string }
  | { kind: "container"; name: string };

export function ActionsPage() {
  const { serverId } = useParams<{ serverId: string }>();
  const navigate = useNavigate();
  const server = useServersStore((s) => s.servers.find((srv) => srv.id === serverId));

  const [services, setServices] = useState<ServiceSummary[]>([]);
  const [servicesLoading, setServicesLoading] = useState(true);
  const [servicesError, setServicesError] = useState<string | null>(null);
  const [serviceFilter, setServiceFilter] = useState("");

  const [containers, setContainers] = useState<ContainerSummary[]>([]);
  const [containersLoading, setContainersLoading] = useState(true);
  const [containersError, setContainersError] = useState<string | null>(null);

  const [confirming, setConfirming] = useState<PendingRestart | null>(null);
  const [restarting, setRestarting] = useState(false);
  const [restartError, setRestartError] = useState<string | null>(null);

  const loadServices = useCallback(() => {
    if (!serverId) return;
    setServicesLoading(true);
    setServicesError(null);
    listServerServices(serverId)
      .then((loaded) => setServices([...loaded].sort((a, b) => a.name.localeCompare(b.name))))
      .catch((err) => setServicesError(err instanceof Error ? err.message : "Couldn't list services."))
      .finally(() => setServicesLoading(false));
  }, [serverId]);

  const loadContainers = useCallback(() => {
    if (!serverId) return;
    setContainersLoading(true);
    setContainersError(null);
    listServerContainers(serverId)
      .then((loaded) => setContainers([...loaded].sort((a, b) => a.name.localeCompare(b.name))))
      .catch((err) => setContainersError(err instanceof Error ? err.message : "Couldn't list containers."))
      .finally(() => setContainersLoading(false));
  }, [serverId]);

  useEffect(loadServices, [loadServices]);
  useEffect(loadContainers, [loadContainers]);

  if (!serverId) {
    return <Navigate to="/servers" replace />;
  }

  const needle = serviceFilter.trim().toLowerCase();
  const filteredServices = needle ? services.filter((s) => s.name.toLowerCase().includes(needle)) : services;

  async function handleConfirmRestart() {
    if (!confirming || !serverId) return;
    setRestarting(true);
    setRestartError(null);
    try {
      if (confirming.kind === "service") {
        await restartServerService(serverId, confirming.name);
        loadServices();
      } else {
        await restartServerContainer(serverId, confirming.name);
        loadContainers();
      }
      toastSuccess(`Restarted ${confirming.name}`);
      setConfirming(null);
    } catch (err) {
      setRestartError(err instanceof Error ? err.message : "Couldn't restart this.");
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

      {servicesError && <p className="page-error-note">{servicesError}</p>}

      <Card title="Systemd services" subtitle={`${services.length} units`}>
        <input
          className="form-input actions-filter"
          placeholder="Filter by name..."
          value={serviceFilter}
          onChange={(e) => setServiceFilter(e.target.value)}
        />
        {servicesLoading ? (
          <SkeletonRows />
        ) : (
          <ul className="server-list">
            {filteredServices.slice(0, MAX_ROWS_SHOWN).map((service) => (
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
                    setConfirming({ kind: "service", name: service.name });
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

      {containersError && <p className="page-error-note">{containersError}</p>}

      <Card title="Docker containers" subtitle={`${containers.length} containers`}>
        {containersLoading ? (
          <SkeletonRows count={3} />
        ) : containers.length === 0 ? (
          <p className="settings-muted">No containers, or Docker isn't installed on this server.</p>
        ) : (
          <ul className="server-list">
            {containers.map((container) => (
              <li key={container.id} className="server-list-item">
                <div className="server-list-main">
                  <span className="server-list-name">{container.name}</span>
                  <span className="server-list-host">
                    {container.image} · {container.status}
                  </span>
                </div>
                <Badge tone={container.running ? "success" : "neutral"}>{container.running ? "Running" : "Stopped"}</Badge>
                <button
                  className="server-list-action"
                  aria-label={`Restart ${container.name}`}
                  onClick={() => {
                    setConfirming({ kind: "container", name: container.name });
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
              <h2 className="modal-title">Restart {confirming.kind === "service" ? "service" : "container"}</h2>
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
