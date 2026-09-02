import { useCallback, useEffect, useState } from "react";
import { Navigate, useNavigate, useParams } from "react-router-dom";
import { Trans, useTranslation } from "react-i18next";
import { POLL_INTERVALS, usePolling } from "@/hooks/usePolling";
import { AskVibeAiButton } from "@/components/ai/AskVibeAiButton";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { RowPicker, serverRowPickerOption } from "@/components/ui/RowPicker";
import { useAiReady } from "@/hooks/useAiReady";
import { useModalDialog } from "@/hooks/useModalDialog";
import { ApplicationBackupsTab } from "@/components/applications/ApplicationBackupsTab";
import { ApplicationConfigCard } from "@/components/applications/ApplicationConfigCard";
import { ApplicationConsoleCard } from "@/components/applications/ApplicationConsoleCard";
import { DatabasesTab } from "@/components/applications/DatabasesTab";
import { DockerImageCard } from "@/components/applications/DockerImageCard";
import { EnvironmentTab } from "@/components/applications/EnvironmentTab";
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
  migrateApplication,
  recreateApplication,
  restartApplication,
  startApplication,
  stopApplication,
} from "@/services/applicationService";
import { useServersStore } from "@/stores/serversStore";
import { toastError, toastSuccess } from "@/stores/toastStore";
import { translateBlueprint } from "@/i18n/blueprintTranslations";
import type { ApplicationDetail as ApplicationDetailData, ApplicationStatus, Blueprint, ResourceUsage } from "@/types/application";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";
import "@/components/applications/CreateApplicationWizard.css";
import "./pages.css";
import "./ApplicationDetail.css";
import { errorMessage } from "@/services/tauri";
import { BlueprintIcon } from "@/components/applications/BlueprintIcon";

const LOG_TAIL_LINES = 500;

type Tab = "overview" | "logs" | "environment" | "ports" | "databases" | "files" | "backups" | "settings";
type Verb = "start" | "stop" | "restart" | "kill" | "recreate";

const STATUS_TONE: Record<ApplicationStatus, "neutral" | "success" | "danger" | "warning"> = {
  unknown: "neutral",
  starting: "warning",
  running: "success",
  stopping: "warning",
  stopped: "neutral",
  failed: "danger",
};

/** stop/kill interrupt or force-end something and read as the "careful" action; start/restart don't - same convention Actions.tsx's VERB_IS_DESTRUCTIVE already establishes for services/containers. */
const VERB_IS_DESTRUCTIVE: Record<Verb, boolean> = { start: false, stop: true, restart: false, kill: true, recreate: false };

export function ApplicationDetail() {
  const { t, i18n } = useTranslation();
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
  const confirmBackdrop = useModalDialog(() => !actionBusy && setConfirming(null), { labelledBy: "applicationdetail-dialog-title-1" });

  const [logs, setLogs] = useState<string[]>([]);
  const [logsLoading, setLogsLoading] = useState(false);
  const [logsError, setLogsError] = useState<string | null>(null);

  const [migrateOpen, setMigrateOpen] = useState(false);
  const [migrateTargetServerId, setMigrateTargetServerId] = useState("");
  const [migrateBusy, setMigrateBusy] = useState(false);
  const [migrateError, setMigrateError] = useState<string | null>(null);
  const migrateBackdrop = useModalDialog(() => !migrateBusy && setMigrateOpen(false), { labelledBy: "applicationdetail-dialog-title-2" });

  const reload = useCallback(() => {
    if (!id) return;
    getApplication(id)
      .then(setApplication)
      .catch((err) => setLoadError(errorMessage(err, t)));
  }, [id, t]);

  useEffect(() => {
    listBlueprints()
      .then((all) => {
        const found = all.find((b) => b.id === application?.blueprintId) ?? null;
        setBlueprint(found ? translateBlueprint(found, i18n.language) : null);
      })
      .catch(() => {});
  }, [application?.blueprintId, i18n.language]);

  const poll = useCallback(async () => {
    if (!id) return;
    try {
      const nextApplication = await getApplication(id);
      setApplication(nextApplication);
      setLoadError(null);
    } catch (err) {
      setLoadError(errorMessage(err, t));
      return;
    }
    // A separate try/catch on purpose - resource usage (a Remote Process
    // over SSH, in particular) can fail on its own (a temporarily
    // unreachable Node) without that meaning the Application itself failed
    // to load. Bundling both into one Promise.all used to throw away an
    // already-successful `getApplication` result and leave the whole page
    // stuck on a bare id with nothing usable on it.
    try {
      setResourceUsage(await getApplicationResourceUsage(id));
    } catch {
      // Leave the last-known usage in place rather than clearing it - this
      // tab already shows its own errors where it matters (Console, Logs,
      // ...), no need for a second banner here.
    }
  }, [id, t]);

  usePolling(poll, POLL_INTERVALS.applicationDetail, { enabled: Boolean(id) });

  const loadLogs = useCallback(() => {
    if (!id) return;
    setLogsLoading(true);
    setLogsError(null);
    getApplicationLogs(id, LOG_TAIL_LINES)
      .then(setLogs)
      .catch((err) => setLogsError(errorMessage(err, t)))
      .finally(() => setLogsLoading(false));
  }, [id, t]);

  useEffect(() => {
    if (tab === "logs") loadLogs();
  }, [tab, loadLogs]);

  /**
   * Runs a lifecycle verb.
   *
   * Split out of the confirmation handler so Start can call it directly.
   * Starting an Application risks nothing and undoes itself with one click,
   * so a modal in front of it was a second click for no decision - unlike
   * Stop and Restart, which drop whoever is connected, or Kill and
   * Recreate, which lose work.
   */
  async function runAction(verb: Verb) {
    if (!id) return;
    setActionBusy(true);
    setActionError(null);
    try {
      const call: Record<Verb, () => Promise<ApplicationStatus>> = {
        start: () => startApplication(id),
        stop: () => stopApplication(id, true),
        restart: () => restartApplication(id),
        kill: () => killApplication(id),
        recreate: () => recreateApplication(id),
      };
      await call[verb]();
      toastSuccess(t(`applicationDetail.verbPast.${verb}`, { name: application?.name ?? "" }));
      setConfirming(null);
      reload();
    } catch (err) {
      setActionError(err instanceof Error ? err.message : t("applicationDetail.couldntDo", { verb: t(`applicationDetail.verb.${verb}`) }));
    } finally {
      setActionBusy(false);
    }
  }

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
        recreate: () => recreateApplication(id),
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

  async function handleMigrate() {
    if (!id || !migrateTargetServerId) return;
    setMigrateBusy(true);
    setMigrateError(null);
    try {
      const result = await migrateApplication(id, migrateTargetServerId);
      // A migration that copied the data but left the old container running,
      // or left the DNS name pointing at it, is not a plain success - and it
      // is the operator, not VibeSSH, who has to finish it.
      if (result.warnings.length > 0) {
        toastError(t("applicationDetail.migrateWarningsToast", { name: result.application.name, warning: result.warnings[0] }));
      } else if (!result.started) {
        toastError(t("applicationDetail.migrateNotStartedToast", { name: result.application.name }));
      } else {
        toastSuccess(t("applicationDetail.migrateSuccessToast", { name: result.application.name }));
      }
      setMigrateOpen(false);
      navigate(`/applications/${result.application.id}`);
    } catch (err) {
      setMigrateError(errorMessage(err, t));
    } finally {
      setMigrateBusy(false);
    }
  }

  if (!id) {
    return <Navigate to="/applications" replace />;
  }

  const aiReady = useAiReady();
  const canStart = application ? ["stopped", "failed", "unknown"].includes(application.status) : false;
  const canStopOrRestart = application ? ["running", "starting"].includes(application.status) : false;
  const serverName = application?.serverId ? (servers.find((s) => s.id === application.serverId)?.name ?? application.serverId) : null;
  const features = blueprint?.features ?? [];
  const migrationTargets = servers.filter((s) => s.id !== application?.serverId);
  const migrateTargetWarning =
    migrateTargetServerId &&
    (servers.find((s) => s.id === migrateTargetServerId)?.connectionMode === "agent"
      ? servers.find((s) => s.id === migrateTargetServerId)?.capabilities?.docker === false
      : servers.find((s) => s.id === migrateTargetServerId)?.nodeCapabilities?.docker === false);

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
              {/* The quick action from the brief: a failed Application is the
                  case where somebody most wants an explanation, and this is
                  where they are already looking. Hidden unless the assistant
                  is configured - see `useAiReady`. */}
              {aiReady && application.status === "failed" && (
                <AskVibeAiButton
                  context={{ kind: "application", id: application.id }}
                  contextLabel={application.name}
                  question={t("vibeAi.seedApplicationFailed", { name: application.name })}
                />
              )}
              {canStart && (
                <Button size="sm" onClick={() => void runAction("start")} disabled={actionBusy}>
                  <Icon name="play" size={14} />
                  {actionBusy ? t("applicationDetail.working.start") : t("applicationDetail.verb.start")}
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
              {application.runtimeType === "docker" && (
                <Button variant="secondary" size="sm" onClick={() => setConfirming("recreate")}>
                  <BlueprintIcon blueprintId={application?.blueprintId} size={14} />
                  {t("applicationDetail.verb.recreate")}
                </Button>
              )}
              {application.runtimeType === "docker" && migrationTargets.length > 0 && (
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => {
                    setMigrateError(null);
                    setMigrateTargetServerId(migrationTargets[0]?.id ?? "");
                    setMigrateOpen(true);
                  }}
                >
                  <Icon name="move" size={14} />
                  {t("applicationDetail.migrateButton")}
                </Button>
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
            <button className={`modal-tab ${tab === "backups" ? "modal-tab-active" : ""}`} onClick={() => setTab("backups")}>
              {t("applicationDetail.tabBackups")}
            </button>
            <button className={`modal-tab ${tab === "settings" ? "modal-tab-active" : ""}`} onClick={() => setTab("settings")}>
              {t("applicationDetail.tabSettings")}
            </button>
          </div>

          {tab === "overview" && (
            <div className="application-detail-overview">
              {features.includes("console") && <ApplicationConsoleCard applicationId={id} isRunning={application.status === "running"} />}

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
            </div>
          )}

          {tab === "settings" && (
            <div className="application-detail-overview">
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

              <ApplicationConfigCard applicationId={id} application={application} blueprint={blueprint} onSaved={reload} />

              {application.runtimeType === "docker" && <DockerImageCard applicationId={id} application={application} onSaved={reload} />}

              {features.includes("healthCheck") && (
                <HealthCheckCard applicationId={id} application={application} onConfigChanged={reload} />
              )}

              {(application.runtimeType === "docker" || application.runtimeType === "systemd" || application.runtimeType === "remoteProcess") && (
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

          {tab === "environment" && <EnvironmentTab application={application} onSaved={reload} />}

          {tab === "ports" && <PortsTab applicationId={id} application={application} />}

          {tab === "databases" && <DatabasesTab applicationId={id} />}

          {tab === "files" && <ApplicationFilesTab applicationId={id} application={application} knownFiles={blueprint?.knownFiles ?? []} />}
          {tab === "backups" && <ApplicationBackupsTab applicationId={id} applicationStatus={application.status} />}
        </>
      )}

      {confirming && (
        <div className="modal-backdrop" {...confirmBackdrop.backdropProps}>
          <div className="modal-panel modal-panel-sm" {...confirmBackdrop.panelProps}>
            <div className="modal-header">
              <h2 className="modal-title" id="applicationdetail-dialog-title-1">{t("applicationDetail.confirmTitle", { verb: t(`applicationDetail.verb.${confirming}`) })}</h2>
              <IconButton icon="x" size="sm" onClick={() => setConfirming(null)} title={t("common.close")} />
            </div>
            <div className="modal-body">
              <p className="dialog-body-text">
                <Trans i18nKey={`applicationDetail.confirmBody.${confirming}`} values={{ name: application?.name ?? "" }} components={{ 1: <strong /> }} />
              </p>
              {actionBusy && (confirming === "stop" || confirming === "restart") && (
                <p className="form-note">{t("applicationDetail.gracefulNote")}</p>
              )}
              {actionError && <p className="form-note form-note-danger form-note-spaced">{actionError}</p>}
              <div className="form-actions">
                <Button variant="secondary" onClick={() => setConfirming(null)} disabled={actionBusy}>
                  {t("common.cancel")}
                </Button>
                <Button variant={VERB_IS_DESTRUCTIVE[confirming] ? "danger" : "primary"} onClick={handleConfirmAction} disabled={actionBusy}>
                  {actionBusy ? t(`applicationDetail.working.${confirming}`) : t(`applicationDetail.verb.${confirming}`)}
                </Button>
              </div>
            </div>
          </div>
        </div>
      )}

      {migrateOpen && (
        <div className="modal-backdrop" {...migrateBackdrop.backdropProps}>
          <div className="modal-panel modal-panel-sm" {...migrateBackdrop.panelProps}>
            <div className="modal-header">
              <h2 className="modal-title" id="applicationdetail-dialog-title-2">{t("applicationDetail.migrateTitle")}</h2>
              <IconButton icon="x" size="sm" onClick={() => setMigrateOpen(false)} title={t("common.close")} />
            </div>
            <div className="modal-body">
              <p className="dialog-body-text">{t("applicationDetail.migrateBody", { name: application?.name ?? "" })}</p>
              <p className="form-note form-note-spaced">{t("applicationDetail.migrateBodyNote")}</p>
              <RowPicker
                label={t("applicationDetail.migrateTargetLabel")}
                placeholder={t("applicationDetail.migrateTargetLabel")}
                value={migrateTargetServerId}
                onChange={setMigrateTargetServerId}
                disabled={migrateBusy}
                options={migrationTargets.map((s) => serverRowPickerOption(s, t))}
              />
              {migrateTargetWarning && <p className="form-note form-note-danger">{t("createApplicationWizard.dockerNotDetected")}</p>}
              {migrateError && <p className="form-note form-note-danger form-note-spaced">{migrateError}</p>}
              <div className="form-actions">
                <Button variant="secondary" onClick={() => setMigrateOpen(false)} disabled={migrateBusy}>
                  {t("common.cancel")}
                </Button>
                <Button variant="danger" onClick={handleMigrate} disabled={migrateBusy || !migrateTargetServerId}>
                  {migrateBusy ? t("applicationDetail.migrating") : t("applicationDetail.migrateConfirm")}
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
