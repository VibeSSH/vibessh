import { useCallback, useEffect, useState } from "react";
import { Navigate, useNavigate, useParams } from "react-router-dom";
import { Trans, useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { useBackdropClose } from "@/hooks/useBackdropClose";
import { ContainerLogsPanel } from "@/components/servers/ContainerLogsPanel";
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
  const { t } = useTranslation();
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

  const [viewingLogsFor, setViewingLogsFor] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<PendingAction | null>(null);
  const [actionBusy, setActionBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const confirmBackdrop = useBackdropClose(() => !actionBusy && setConfirming(null));

  const loadServices = useCallback(() => {
    if (!serverId) return;
    setServicesLoading(true);
    setServicesError(null);
    listServerServices(serverId)
      .then((loaded) => setServices([...loaded].sort((a, b) => a.name.localeCompare(b.name))))
      .catch((err) => setServicesError(err instanceof Error ? err.message : t("actionsPage.couldntListServices")))
      .finally(() => setServicesLoading(false));
  }, [serverId]);

  const loadContainers = useCallback(() => {
    if (!serverId) return;
    setContainersLoading(true);
    setContainersError(null);
    listServerContainers(serverId)
      .then((loaded) => setContainers([...loaded].sort((a, b) => a.name.localeCompare(b.name))))
      .catch((err) => setContainersError(err instanceof Error ? err.message : t("actionsPage.couldntListContainers")))
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
      toastSuccess(t("actionsPage.toast", { verb: t(`actionsPage.verbPast.${confirming.verb}`), name: confirming.name }));
      setConfirming(null);
    } catch (err) {
      setActionError(err instanceof Error ? err.message : t("actionsPage.couldntDo", { verb: t(`actionsPage.verb.${confirming.verb}`) }));
    } finally {
      setActionBusy(false);
    }
  }

  return (
    <div className="page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">{server ? server.name : t("nav.actions")}</h1>
          <p className="page-subtitle">{server ? server.host : serverId}</p>
        </div>
        <Button variant="secondary" onClick={() => navigate("/servers")}>
          <Icon name="chevron-left" size={16} />
          {t("common.backToServers")}
        </Button>
      </div>

      {servicesError && <p className="page-error-note">{servicesError}</p>}

      <Card title={t("actionsPage.servicesTitle")} subtitle={t("actionsPage.unitsCount", { count: services.length })}>
        <input
          className="form-input actions-filter"
          placeholder={t("actionsPage.filterByName")}
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
                  <span className="server-list-name" title={service.name}>{service.name}</span>
                  <span className="server-list-host" title={service.description}>{service.description}</span>
                </div>
                <Badge tone={service.active ? "success" : "neutral"}>{service.active ? t("actionsPage.active") : t("actionsPage.inactive")}</Badge>
                <Badge tone="neutral">{service.enabled ? t("actionsPage.enabled") : t("actionsPage.disabled")}</Badge>
                <div className="server-list-actions">
                  <button
                    className="server-list-action"
                    title={service.active ? t("actionsPage.stopAria", { name: service.name }) : t("actionsPage.startAria", { name: service.name })}
                    aria-label={service.active ? t("actionsPage.stopAria", { name: service.name }) : t("actionsPage.startAria", { name: service.name })}
                    onClick={() => askConfirm({ kind: "service", name: service.name, verb: service.active ? "stop" : "start" })}
                  >
                    <Icon name={service.active ? "square" : "play"} size={14} />
                  </button>
                  <button
                    className="server-list-action"
                    title={t("actionsPage.restartAria", { name: service.name })}
                    aria-label={t("actionsPage.restartAria", { name: service.name })}
                    onClick={() => askConfirm({ kind: "service", name: service.name, verb: "restart" })}
                  >
                    <Icon name="zap" size={14} />
                  </button>
                  <button
                    className="server-list-action"
                    title={
                      service.enabled ? t("actionsPage.disableAria", { name: service.name }) : t("actionsPage.enableAria", { name: service.name })
                    }
                    aria-label={
                      service.enabled ? t("actionsPage.disableAria", { name: service.name }) : t("actionsPage.enableAria", { name: service.name })
                    }
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

      <Card title={t("actionsPage.containersTitle")} subtitle={t("actionsPage.containersCount", { count: containers.length })}>
        {containersLoading ? (
          <SkeletonRows count={3} />
        ) : containers.length === 0 ? (
          <p className="settings-muted">{t("actionsPage.noContainers")}</p>
        ) : (
          <ul className="server-list">
            {containers.map((container) => (
              <li key={container.id} className="server-list-item">
                <div className="server-list-main">
                  <span className="server-list-name" title={container.name}>{container.name}</span>
                  <span className="server-list-host" title={`${container.image} · ${container.status}`}>
                    {container.image} · {container.status}
                  </span>
                </div>
                <Badge tone={container.running ? "success" : "neutral"}>{container.running ? t("actionsPage.running") : t("actionsPage.stopped")}</Badge>
                <div className="server-list-actions">
                  <button
                    className="server-list-action"
                    title={t("actionsPage.viewLogsAria", { name: container.name })}
                    aria-label={t("actionsPage.viewLogsAria", { name: container.name })}
                    onClick={() => setViewingLogsFor(container.name)}
                  >
                    <Icon name="terminal" size={14} />
                  </button>
                  <button
                    className="server-list-action"
                    title={
                      container.running ? t("actionsPage.stopAria", { name: container.name }) : t("actionsPage.startAria", { name: container.name })
                    }
                    aria-label={
                      container.running ? t("actionsPage.stopAria", { name: container.name }) : t("actionsPage.startAria", { name: container.name })
                    }
                    onClick={() =>
                      askConfirm({ kind: "container", name: container.name, verb: container.running ? "stop" : "start" })
                    }
                  >
                    <Icon name={container.running ? "square" : "play"} size={14} />
                  </button>
                  <button
                    className="server-list-action"
                    title={t("actionsPage.restartAria", { name: container.name })}
                    aria-label={t("actionsPage.restartAria", { name: container.name })}
                    onClick={() => askConfirm({ kind: "container", name: container.name, verb: "restart" })}
                  >
                    <Icon name="zap" size={14} />
                  </button>
                  <button
                    className="server-list-action"
                    title={t("actionsPage.removeAria", { name: container.name })}
                    aria-label={t("actionsPage.removeAria", { name: container.name })}
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

      {viewingLogsFor && (
        <ContainerLogsPanel serverId={serverId} containerName={viewingLogsFor} onClose={() => setViewingLogsFor(null)} />
      )}

      {confirming && (
        <div className="modal-backdrop" {...confirmBackdrop}>
          <div className="modal-panel modal-panel-sm" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2 className="modal-title">
                {t(confirming.kind === "service" ? "actionsPage.confirmTitleService" : "actionsPage.confirmTitleContainer", {
                  verb: t(`actionsPage.verb.${confirming.verb}`),
                })}
              </h2>
              <IconButton icon="x" size="sm" onClick={() => setConfirming(null)} title={t("common.close")} />
            </div>
            <div className="modal-body">
              <p className="dialog-body-text">
                <Trans i18nKey={`actionsPage.confirmBody.${confirming.verb}`} values={{ name: confirming.name }} components={{ 1: <strong /> }} />
              </p>
              {actionError && <p className="form-note form-note-danger form-note-spaced">{actionError}</p>}
              <div className="form-actions">
                <Button variant="secondary" onClick={() => setConfirming(null)} disabled={actionBusy}>
                  {t("common.cancel")}
                </Button>
                <Button
                  variant={VERB_IS_DESTRUCTIVE[confirming.verb] ? "danger" : "primary"}
                  onClick={handleConfirmAction}
                  disabled={actionBusy}
                >
                  {t(`actionsPage.verb.${confirming.verb}`)}
                </Button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
