import { useCallback, useEffect, useState } from "react";
import { Navigate, useNavigate, useParams } from "react-router-dom";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import {
  disableServerService,
  enableServerService,
  listServerContainers,
  listServerServices,
  removeServerContainer,
  restartServerContainer,
  restartServerService,
  startServerContainer,
  startServerService,
  stopServerContainer,
  stopServerService,
} from "@/services/actionsService";
import { useServersStore } from "@/stores/serversStore";
import { toastSuccess } from "@/stores/toastStore";
import type { ContainerSummary, ServiceSummary } from "@/types/serverEvent";
import "./pages.css";
import "./Actions.css";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";

const MAX_ROWS_SHOWN = 200;

type ServiceVerb = "start" | "stop" | "restart" | "enable" | "disable";
type ContainerVerb = "start" | "stop" | "restart" | "remove";

type PendingAction =
  | { kind: "service"; name: string; verb: ServiceVerb }
  | { kind: "container"; name: string; verb: ContainerVerb };

const VERB_LABEL: Record<ServiceVerb | ContainerVerb, string> = {
  start: "Start",
  stop: "Stop",
  restart: "Restart",
  enable: "Enable",
  disable: "Disable",
  remove: "Remove",
};

const VERB_PAST: Record<ServiceVerb | ContainerVerb, string> = {
  start: "Started",
  stop: "Stopped",
  restart: "Restarted",
  enable: "Enabled",
  disable: "Disabled",
  remove: "Removed",
};

const VERB_BODY: Record<ServiceVerb | ContainerVerb, string> = {
  start: "Start {name}?",
  stop: "Stop {name}? Anything using it loses its connection.",
  restart: "Restart {name}? Anything using it may briefly disconnect.",
  enable: "Enable {name}? It will start automatically on boot.",
  disable: "Disable {name}? It won't start automatically on boot anymore.",
  remove: "Remove {name}? This deletes the container, not just stops it - it can't be undone.",
};

/** stop/disable/remove interrupt or end something and read as the "careful" action; start/restart/enable don't. */
const VERB_IS_DESTRUCTIVE: Record<ServiceVerb | ContainerVerb, boolean> = {
  start: false,
  stop: true,
  restart: false,
  enable: false,
  disable: true,
  remove: true,
};

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

  const [confirming, setConfirming] = useState<PendingAction | null>(null);
  const [actionBusy, setActionBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

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

  function askConfirm(action: PendingAction) {
    setConfirming(action);
    setActionError(null);
  }

  async function handleConfirmAction() {
    if (!confirming || !serverId) return;
    setActionBusy(true);
    setActionError(null);
    try {
      if (confirming.kind === "service") {
        const call = {
          start: startServerService,
          stop: stopServerService,
          restart: restartServerService,
          enable: enableServerService,
          disable: disableServerService,
        }[confirming.verb];
        await call(serverId, confirming.name);
        loadServices();
      } else {
        const call = {
          start: startServerContainer,
          stop: stopServerContainer,
          restart: restartServerContainer,
          remove: removeServerContainer,
        }[confirming.verb];
        await call(serverId, confirming.name);
        loadContainers();
      }
      toastSuccess(`${VERB_PAST[confirming.verb]} ${confirming.name}`);
      setConfirming(null);
    } catch (err) {
      setActionError(err instanceof Error ? err.message : `Couldn't ${confirming.verb} this.`);
    } finally {
      setActionBusy(false);
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
                <div className="server-list-actions">
                  <button
                    className="server-list-action"
                    aria-label={service.active ? `Stop ${service.name}` : `Start ${service.name}`}
                    onClick={() => askConfirm({ kind: "service", name: service.name, verb: service.active ? "stop" : "start" })}
                  >
                    <Icon name={service.active ? "square" : "play"} size={14} />
                  </button>
                  <button
                    className="server-list-action"
                    aria-label={`Restart ${service.name}`}
                    onClick={() => askConfirm({ kind: "service", name: service.name, verb: "restart" })}
                  >
                    <Icon name="zap" size={14} />
                  </button>
                  <button
                    className="server-list-action"
                    aria-label={service.enabled ? `Disable ${service.name}` : `Enable ${service.name}`}
                    onClick={() => askConfirm({ kind: "service", name: service.name, verb: service.enabled ? "disable" : "enable" })}
                  >
                    <Icon name="power" size={14} />
                  </button>
                </div>
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
                <div className="server-list-actions">
                  <button
                    className="server-list-action"
                    aria-label={container.running ? `Stop ${container.name}` : `Start ${container.name}`}
                    onClick={() =>
                      askConfirm({ kind: "container", name: container.name, verb: container.running ? "stop" : "start" })
                    }
                  >
                    <Icon name={container.running ? "square" : "play"} size={14} />
                  </button>
                  <button
                    className="server-list-action"
                    aria-label={`Restart ${container.name}`}
                    onClick={() => askConfirm({ kind: "container", name: container.name, verb: "restart" })}
                  >
                    <Icon name="zap" size={14} />
                  </button>
                  <button
                    className="server-list-action"
                    aria-label={`Remove ${container.name}`}
                    onClick={() => askConfirm({ kind: "container", name: container.name, verb: "remove" })}
                  >
                    <Icon name="trash" size={14} />
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </Card>

      {confirming && (
        <div className="modal-backdrop" onClick={() => !actionBusy && setConfirming(null)}>
          <div className="modal-panel" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2 className="modal-title">
                {VERB_LABEL[confirming.verb]} {confirming.kind === "service" ? "service" : "container"}
              </h2>
              <button className="modal-close" onClick={() => setConfirming(null)} aria-label="Close">
                <Icon name="x" size={16} />
              </button>
            </div>
            <div className="modal-body">
              <p style={{ margin: "0 0 16px", fontSize: 13, color: "var(--text-primary)", lineHeight: 1.5 }}>
                {VERB_BODY[confirming.verb].split("{name}")[0]}
                <strong>{confirming.name}</strong>
                {VERB_BODY[confirming.verb].split("{name}")[1]}
              </p>
              {actionError && (
                <p className="form-note" style={{ color: "var(--danger)", marginBottom: 12 }}>
                  {actionError}
                </p>
              )}
              <div className="form-actions" style={{ gap: 8 }}>
                <Button variant="secondary" onClick={() => setConfirming(null)} disabled={actionBusy}>
                  Cancel
                </Button>
                <Button
                  variant={VERB_IS_DESTRUCTIVE[confirming.verb] ? "danger" : "primary"}
                  onClick={handleConfirmAction}
                  disabled={actionBusy}
                >
                  {VERB_LABEL[confirming.verb]}
                </Button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
