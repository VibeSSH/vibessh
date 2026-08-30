import { Fragment, useCallback, useEffect, useState } from "react";
import { Navigate, useNavigate, useParams } from "react-router-dom";
import { Trans, useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { useBackdropClose } from "@/hooks/useBackdropClose";
import { DatabasesTab } from "@/components/applications/DatabasesTab";
import { ApplicationFilesTab } from "@/components/applications/files/ApplicationFilesTab";
import { HealthCheckCard } from "@/components/applications/HealthCheckCard";
import { PortsTab } from "@/components/applications/PortsTab";
import { ResourceLimitsCard } from "@/components/applications/ResourceLimitsCard";
import {
  getApplication,
  getApplicationLogs,
  getApplicationResourceUsage,
  killApplication,
  listBlueprints,
  restartApplication,
  startApplication,
  stopApplication,
} from "@/services/applicationService";
import { useServersStore } from "@/stores/serversStore";
import { toastSuccess } from "@/stores/toastStore";
import type { ApplicationDetail as ApplicationDetailData, ApplicationStatus, Blueprint, ResourceUsage } from "@/types/application";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";
import "@/components/applications/CreateApplicationWizard.css";
import "./pages.css";
import "./ApplicationDetail.css";

const POLL_INTERVAL_MS = 5000;
const LOG_TAIL_LINES = 500;

type Tab = "overview" | "logs" | "environment" | "ports" | "databases" | "files";
type Verb = "start" | "stop" | "restart" | "kill";

const STATUS_TONE: Record<ApplicationStatus, "neutral" | "success" | "danger" | "warning"> = {
  unknown: "neutral",
  starting: "warning",
  running: "success",
  stopping: "warning",
  stopped: "neutral",
  failed: "danger",
};

/** stop/kill interrupt or force-end something and read as the "careful" action; start/restart don't - same convention Actions.tsx's VERB_IS_DESTRUCTIVE already establishes for services/containers. */
const VERB_IS_DESTRUCTIVE: Record<Verb, boolean> = { start: false, stop: true, restart: false, kill: true };

export function ApplicationDetail() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { id } = useParams<{ id: string }>();
  const servers = useServersStore((s) => s.servers);

  const [application, setApplication] = useState<ApplicationDetailData | null>(null);
  const [blueprint, setBlueprint] = useState<Blueprint | null>(null);
  const [resourceUsage, setResourceUsage] = useState<ResourceUsage | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("overview");

  const [confirming, setConfirming] = useState<Verb | null>(null);
  const [actionBusy, setActionBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const confirmBackdrop = useBackdropClose(() => !actionBusy && setConfirming(null));

  const [logs, setLogs] = useState<string[]>([]);
  const [logsLoading, setLogsLoading] = useState(false);
  const [logsError, setLogsError] = useState<string | null>(null);

  const reload = useCallback(() => {
    if (!id) return;
    getApplication(id)
      .then(setApplication)
      .catch((err) => setLoadError(err instanceof Error ? err.message : t("applicationDetail.loadError")));
  }, [id, t]);

  useEffect(() => {
    listBlueprints()
      .then((all) => setBlueprint(all.find((b) => b.id === application?.blueprintId) ?? null))
      .catch(() => {});
  }, [application?.blueprintId]);

  useEffect(() => {
    if (!id) return;
    let cancelled = false;

    async function poll() {
      try {
        const [nextApplication, nextUsage] = await Promise.all([getApplication(id!), getApplicationResourceUsage(id!)]);
        if (cancelled) return;
        setApplication(nextApplication);
        setResourceUsage(nextUsage);
        setLoadError(null);
      } catch (err) {
        if (cancelled) return;
        setLoadError(err instanceof Error ? err.message : t("applicationDetail.loadError"));
      }
    }

    poll();
    const intervalId = window.setInterval(poll, POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(intervalId);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  const loadLogs = useCallback(() => {
    if (!id) return;
    setLogsLoading(true);
    setLogsError(null);
    getApplicationLogs(id, LOG_TAIL_LINES)
      .then(setLogs)
      .catch((err) => setLogsError(err instanceof Error ? err.message : t("applicationDetail.logsError")))
      .finally(() => setLogsLoading(false));
  }, [id, t]);

  useEffect(() => {
    if (tab === "logs") loadLogs();
  }, [tab, loadLogs]);

  async function handleConfirmAction() {
    if (!confirming || !id) return;
    setActionBusy(true);
    setActionError(null);
    try {
      const call: Record<Verb, () => Promise<ApplicationStatus>> = {
        start: () => startApplication(id),
        stop: () => stopApplication(id, true),
        restart: () => restartApplication(id),
        kill: () => killApplication(id),
      };
      await call[confirming]();
      toastSuccess(t(`applicationDetail.verbPast.${confirming}`, { name: application?.name ?? "" }));
      setConfirming(null);
      reload();
    } catch (err) {
      setActionError(err instanceof Error ? err.message : t("applicationDetail.couldntDo", { verb: t(`applicationDetail.verb.${confirming}`) }));
    } finally {
      setActionBusy(false);
    }
  }

  if (!id) {
    return <Navigate to="/applications" replace />;
  }

  const canStart = application ? ["stopped", "failed", "unknown"].includes(application.status) : false;
  const canStopOrRestart = application ? ["running", "starting"].includes(application.status) : false;
  const serverName = application?.serverId ? (servers.find((s) => s.id === application.serverId)?.name ?? application.serverId) : null;
  const features = blueprint?.features ?? [];

  return (
    <div className="page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">{application ? application.name : id}</h1>
          <p className="page-subtitle">{blueprint ? blueprint.name : application?.blueprintId}</p>
        </div>
        <Button variant="secondary" onClick={() => navigate("/applications")}>
          <Icon name="chevron-left" size={16} />
          {t("applications.backToApplications")}
        </Button>
      </div>

      {loadError && <p className="page-error-note">{loadError}</p>}

      {application && (
        <>
          <div className="application-detail-header-row">
            <Badge tone={STATUS_TONE[application.status]}>{t(`applicationStatus.${application.status}`)}</Badge>
            <div className="application-detail-actions">
              {canStart && (
                <Button size="sm" onClick={() => setConfirming("start")}>
                  <Icon name="play" size={14} />
                  {t("applicationDetail.verb.start")}
                </Button>
              )}
              {canStopOrRestart && (
                <>
                  <Button variant="secondary" size="sm" onClick={() => setConfirming("stop")}>
                    <Icon name="square" size={14} />
                    {t("applicationDetail.verb.stop")}
                  </Button>
                  <Button variant="secondary" size="sm" onClick={() => setConfirming("restart")}>
                    <Icon name="refresh-cw" size={14} />
                    {t("applicationDetail.verb.restart")}
                  </Button>
                  <Button variant="danger" size="sm" onClick={() => setConfirming("kill")}>
                    <Icon name="power" size={14} />
                    {t("applicationDetail.verb.kill")}
                  </Button>
                </>
              )}
            </div>
          </div>

          <div className="page-tabs">
            <button className={`modal-tab ${tab === "overview" ? "modal-tab-active" : ""}`} onClick={() => setTab("overview")}>
              {t("applicationDetail.tabOverview")}
            </button>
            {features.includes("logs") && (
              <button className={`modal-tab ${tab === "logs" ? "modal-tab-active" : ""}`} onClick={() => setTab("logs")}>
                {t("applicationDetail.tabLogs")}
              </button>
            )}
            {features.includes("environment") && (
              <button className={`modal-tab ${tab === "environment" ? "modal-tab-active" : ""}`} onClick={() => setTab("environment")}>
                {t("applicationDetail.tabEnvironment")}
              </button>
            )}
            {features.includes("ports") && (
              <button className={`modal-tab ${tab === "ports" ? "modal-tab-active" : ""}`} onClick={() => setTab("ports")}>
                {t("applicationDetail.tabPorts")}
              </button>
            )}
            {features.includes("databases") && (
              <button className={`modal-tab ${tab === "databases" ? "modal-tab-active" : ""}`} onClick={() => setTab("databases")}>
                {t("applicationDetail.tabDatabases")}
              </button>
            )}
            {features.includes("files") && (
              <button className={`modal-tab ${tab === "files" ? "modal-tab-active" : ""}`} onClick={() => setTab("files")}>
                {t("applicationDetail.tabFiles")}
              </button>
            )}
          </div>

          {tab === "overview" && (
            <div className="application-detail-overview">
              <Card title={t("applicationDetail.resourceUsageTitle")}>
                {application.status === "running" && resourceUsage ? (
                  <div className="stat-grid">
                    <div>
                      <p className="form-label">{t("applicationDetail.cpu")}</p>
                      <p className="application-detail-stat-value">{resourceUsage.cpuPercent?.toFixed(1) ?? "—"}%</p>
                    </div>
                    <div>
                      <p className="form-label">{t("applicationDetail.ram")}</p>
                      <p className="application-detail-stat-value">
                        {resourceUsage.ramBytes ? `${(resourceUsage.ramBytes / 1024 / 1024).toFixed(0)} MB` : "—"}
                      </p>
                    </div>
                    <div>
                      <p className="form-label">{t("applicationDetail.uptime")}</p>
                      <p className="application-detail-stat-value">{resourceUsage.uptimeSeconds ? formatUptime(resourceUsage.uptimeSeconds) : "—"}</p>
                    </div>
                  </div>
                ) : (
                  <p className="form-note">{t("applicationDetail.notRunning")}</p>
                )}
              </Card>

              <Card title={t("applicationDetail.detailsTitle")}>
                <div className="wizard-review-grid">
                  <span className="wizard-review-label">{t("createApplicationWizard.location")}</span>
                  <span className="wizard-review-value">{serverName ?? t("applicationCard.local")}</span>
                  <span className="wizard-review-label">{t("createApplicationWizard.runtimeType")}</span>
                  <span className="wizard-review-value">{t(`createApplicationWizard.runtimeTypeOption.${application.runtimeType}`)}</span>
                  <span className="wizard-review-label">{t("createApplicationWizard.workingDirectory")}</span>
                  <span className="wizard-review-value">{application.workingDirectory}</span>
                  <span className="wizard-review-label">{t("applicationDetail.createdAt")}</span>
                  <span className="wizard-review-value">{new Date(application.createdAt).toLocaleString()}</span>
                </div>
              </Card>

              {features.includes("healthCheck") && (
                <HealthCheckCard applicationId={id} application={application} onConfigChanged={reload} />
              )}

              {(application.runtimeType === "docker" || application.runtimeType === "systemd") && (
                <ResourceLimitsCard applicationId={id} application={application} onSaved={reload} />
              )}
            </div>
          )}

          {tab === "logs" && (
            <Card>
              {logsError && <p className="form-note form-note-danger form-note-spaced">{logsError}</p>}
              <pre className="container-logs-output">
                {logsLoading ? t("applicationDetail.logsLoading") : logs.join("\n") || t("applicationDetail.logsEmpty")}
              </pre>
              <div className="form-actions">
                <Button variant="secondary" onClick={loadLogs} disabled={logsLoading}>
                  <Icon name="refresh-cw" size={14} />
                  {t("common.refresh")}
                </Button>
              </div>
            </Card>
          )}

          {tab === "environment" && (
            <Card>
              {application.environment.length === 0 ? (
                <p className="form-note">{t("applicationDetail.environmentEmpty")}</p>
              ) : (
                <div className="wizard-review-grid">
                  {application.environment.map((row) => (
                    <Fragment key={row.key}>
                      <span className="wizard-review-label">{row.key}</span>
                      <span className="wizard-review-value">{row.value}</span>
                    </Fragment>
                  ))}
                </div>
              )}
            </Card>
          )}

          {tab === "ports" && <PortsTab applicationId={id} />}

          {tab === "databases" && <DatabasesTab applicationId={id} />}

          {tab === "files" && <ApplicationFilesTab applicationId={id} application={application} knownFiles={blueprint?.knownFiles ?? []} />}
        </>
      )}

      {confirming && (
        <div className="modal-backdrop" {...confirmBackdrop}>
          <div className="modal-panel modal-panel-sm" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2 className="modal-title">{t("applicationDetail.confirmTitle", { verb: t(`applicationDetail.verb.${confirming}`) })}</h2>
              <IconButton icon="x" size="sm" onClick={() => setConfirming(null)} title={t("common.close")} />
            </div>
            <div className="modal-body">
              <p className="dialog-body-text">
                <Trans i18nKey={`applicationDetail.confirmBody.${confirming}`} values={{ name: application?.name ?? "" }} components={{ 1: <strong /> }} />
              </p>
              {actionError && <p className="form-note form-note-danger form-note-spaced">{actionError}</p>}
              <div className="form-actions">
                <Button variant="secondary" onClick={() => setConfirming(null)} disabled={actionBusy}>
                  {t("common.cancel")}
                </Button>
                <Button variant={VERB_IS_DESTRUCTIVE[confirming] ? "danger" : "primary"} onClick={handleConfirmAction} disabled={actionBusy}>
                  {t(`applicationDetail.verb.${confirming}`)}
                </Button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function formatUptime(totalSeconds: number): string {
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = Math.floor(totalSeconds % 60);
  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m ${seconds}s`;
  return `${seconds}s`;
}
