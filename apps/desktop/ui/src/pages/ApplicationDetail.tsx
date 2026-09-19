import { TabUnderline } from "@/components/ui/TabUnderline";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Navigate, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { Trans, useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { POLL_INTERVALS } from "@/hooks/usePolling";
import { queryKeys } from "@/services/queryKeys";
import { AskVibeAiButton } from "@/components/ai/AskVibeAiButton";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { ApplicationTabs } from "@/components/applications/ApplicationTabs";
import { useApplicationTabsStore } from "@/stores/applicationTabsStore";
import { IconButton } from "@/components/ui/IconButton";
import { RowPicker, serverRowPickerOption } from "@/components/ui/RowPicker";
import { Sparkline } from "@/components/ui/Sparkline";
import { useAiReady } from "@/hooks/useAiReady";
import { useModalDialog } from "@/hooks/useModalDialog";
import { ApplicationBackupsTab } from "@/components/applications/ApplicationBackupsTab";
import { ApplicationConfigCard } from "@/components/applications/ApplicationConfigCard";
import { BlueprintSwitchCard } from "@/components/applications/BlueprintSwitchCard";
import { CommandConsoleCard } from "@/components/applications/CommandConsoleCard";
import { GuideLink } from "@/guide/GuideLink";
import { ApplicationConsoleCard } from "@/components/applications/ApplicationConsoleCard";
import { DatabasesTab } from "@/components/applications/DatabasesTab";
import { DockerImageCard } from "@/components/applications/DockerImageCard";
import { EnvironmentTab } from "@/components/applications/EnvironmentTab";
import { ApplicationFilesTab } from "@/components/applications/files/ApplicationFilesTab";
import { HealthCheckCard } from "@/components/applications/HealthCheckCard";
import { MinecraftMetricsCard } from "@/components/applications/MinecraftMetricsCard";
import { PortsTab } from "@/components/applications/PortsTab";
import { ResourceLimitsCard } from "@/components/applications/ResourceLimitsCard";
import {
  getApplication,
  clearApplicationLogs,
  getApplicationLogs,
  getApplicationResourceUsage,
  killApplication,
  listBlueprints,
  migrateApplication,
  recreateApplication,
  renameApplication,
  restartApplication,
  startApplication,
  stopApplication,
} from "@/services/applicationService";
import { useServersStore } from "@/stores/serversStore";
import { useCanOnServer } from "@/stores/nodePermissionsStore";
import { toastError, toastSuccess } from "@/stores/toastStore";
import { translateBlueprint } from "@/i18n/blueprintTranslations";
import type { ApplicationStatus, Blueprint } from "@/types/application";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";
import "@/components/applications/CreateApplicationWizard.css";
import "./pages.css";
import "./ApplicationDetail.css";
import { errorMessage } from "@/services/tauri";
import { BlueprintIcon } from "@/components/applications/BlueprintIcon";

/** How many resource samples the charts keep - five minutes at
 * `POLL_INTERVALS.applicationDetail`. */
const HISTORY_SAMPLES = 60;

const LOG_TAIL_LINES = 500;

/** Every tab, and the values the `?tab=` parameter accepts. One list, so a
 * tab cannot exist without being linkable to. */
const TABS = ["overview", "files", "logs", "ports", "databases", "backups", "settings"] as const;
type Tab = (typeof TABS)[number];
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

  const queryClient = useQueryClient();
  const [blueprint, setBlueprint] = useState<Blueprint | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameBusy, setRenameBusy] = useState(false);
  const [renameError, setRenameError] = useState<string | null>(null);
  /**
   * Recent samples, for the charts under the console.
   *
   * Held in memory for as long as the page is open and no longer. Nothing
   * persists them, so the charts start empty and fill over the next few
   * minutes rather than showing history that was never recorded - the same
   * bargain any live meter makes, and better than implying a past this app
   * does not have.
   */
  const [history, setHistory] = useState<{ cpu: number; ram: number }[]>([]);
  /**
   * Which tab is open, held in the URL rather than in component state.
   *
   * It was `useState`, which meant a tab could not be linked to: the guide
   * could send somebody to this page but not to the Ports on it, the back
   * button did not undo a tab change, and a reload always landed on
   * Overview. It is also what makes a tab screenshottable at all, since a
   * capture can only be pointed at a URL.
   *
   * An unknown or missing value reads as Overview rather than as an error -
   * a mistyped link should land somewhere sensible.
   */
  const [searchParams, setSearchParams] = useSearchParams();
  const requestedTab = searchParams.get("tab");
  const tab: Tab = TABS.includes(requestedTab as Tab) ? (requestedTab as Tab) : "overview";
  // `replace`, so reading through an application's tabs does not fill the
  // history with every tab glanced at on the way.
  const setTab = (next: Tab) => setSearchParams({ tab: next }, { replace: true });

  const [confirming, setConfirming] = useState<Verb | null>(null);
  const [actionBusy, setActionBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const confirmBackdrop = useModalDialog(() => !actionBusy && setConfirming(null), { labelledBy: "applicationdetail-dialog-title-1" });

  const [logs, setLogs] = useState<string[]>([]);
  const [logsLoading, setLogsLoading] = useState(false);
  const [logsError, setLogsError] = useState<string | null>(null);
  /** The scrolling element itself, so "go to the beginning" can act on it -
   *  500 lines of a crashed server's output is a long way to drag a
   *  scrollbar, and the answer is nearly always at the top of them. */
  const logsRef = useRef<HTMLPreElement>(null);
  const [clearingLogs, setClearingLogs] = useState(false);
  const [clearLogsBusy, setClearLogsBusy] = useState(false);

  const [migrateOpen, setMigrateOpen] = useState(false);
  const [migrateTargetServerId, setMigrateTargetServerId] = useState("");
  const [migrateBusy, setMigrateBusy] = useState(false);
  const [migrateError, setMigrateError] = useState<string | null>(null);
  const migrateBackdrop = useModalDialog(() => !migrateBusy && setMigrateOpen(false), { labelledBy: "applicationdetail-dialog-title-2" });
  const clearLogsBackdrop = useModalDialog(() => !clearLogsBusy && setClearingLogs(false), { labelledBy: "applicationdetail-dialog-title-3" });

  /**
   * The application itself, from the cache.
   *
   * This page used to hold it in `useState` and fetch it on every mount, so
   * opening an Application you were just looking at showed a skeleton while
   * the same question went out again. Now it paints from the cache and
   * refreshes behind that.
   *
   * `refetchInterval` replaces the manual poll. It also makes the two reads
   * genuinely independent - separate queries, each on its own clock, neither
   * waiting for the other or able to fail the other.
   */
  const applicationQuery = useQuery({
    queryKey: queryKeys.application(id ?? ""),
    queryFn: () => getApplication(id as string),
    enabled: Boolean(id),
    refetchInterval: POLL_INTERVALS.applicationDetail,
  });
  const application = applicationQuery.data ?? null;

  // Opening an Application puts it in the strip. Done here rather than at
  // the click that navigated, because an Application can be reached from a
  // link, a quick action or a pasted URL, and all of them should leave a tab
  // behind.
  const openTab = useApplicationTabsStore((state) => state.open);
  // The name the strip already knows, used while the Application itself is
  // still loading. Without it the title showed the raw id for a moment on
  // every switch, which is the whole of what made switching feel abrupt.
  //
  // Deliberately only the *name*. Carrying the previous Application's status
  // and ports across would look smoother still and would be dangerous: the
  // buttons act on the id in the URL, so a stale "Running" under a tab you
  // have already switched away from invites stopping the wrong server.
  const knownName = useApplicationTabsStore((state) => state.tabs.find((tab) => tab.id === id)?.name);
  const rememberedTab = useApplicationTabsStore((state) => state.tabs.find((tab) => tab.id === id)?.lastTab);
  const rememberTab = useApplicationTabsStore((state) => state.rememberTab);

  /**
   * Arriving with no tab in the address lands where this Application was
   * left, not on Overview.
   *
   * The strip's own links carry an id and nothing else, so every hop between
   * two Applications used to reset the view - which is the complaint: copying
   * a value from one into the other meant walking the same three clicks back
   * every trip.
   *
   * Written into the address rather than held beside it, so everything the
   * URL already buys stays true: the back button, a reload, a link to a tab,
   * a screenshot pointed at one. `replace`, because a restored tab is where
   * you already were, not a place you navigated to.
   */
  useEffect(() => {
    if (requestedTab !== null) return;
    if (!rememberedTab || !TABS.includes(rememberedTab as Tab) || rememberedTab === "overview") return;
    setSearchParams({ tab: rememberedTab }, { replace: true });
  }, [requestedTab, rememberedTab, setSearchParams]);

  useEffect(() => {
    if (id) rememberTab(id, tab);
  }, [id, tab, rememberTab]);
  useEffect(() => {
    if (id && application) openTab({ id, name: application.name });
  }, [id, application, openTab]);

  /**
   * Resource usage, on its own query.
   *
   * Its command is `docker stats --no-stream`, which waits for Docker's own
   * sampling and is the slowest single thing this app asks a Node for. On
   * its own key it can be slow, or fail on a temporarily unreachable Node,
   * without holding up or blanking the page around it - which is what
   * happened when both reads shared one sequential poll.
   *
   * Only while the Application is running: `docker stats` on a stopped
   * container is a round trip whose answer is always nothing.
   */
  const usageQuery = useQuery({
    queryKey: [...queryKeys.application(id ?? ""), "usage"],
    queryFn: () => getApplicationResourceUsage(id as string),
    enabled: Boolean(id) && application?.status === "running",
    refetchInterval: POLL_INTERVALS.applicationDetail,
  });
  // Gated on the status, not on the query. A disabled query keeps its last
  // answer, so without this a container you just stopped would keep showing
  // the CPU and memory it was using while it ran.
  const resourceUsage = application?.status === "running" ? (usageQuery.data ?? null) : null;

  const loadError = applicationQuery.error ? errorMessage(applicationQuery.error, t) : null;

  /**
   * Team guard rails for this application's Node.
   *
   * Hides the actions a member has not been given, so nobody presses one by
   * accident. Not a security boundary - the operations run over the
   * operator's own SSH connection from their own machine, so anybody who
   * can reach the Node can do the same thing outside VibeSSH. See
   * `nodePermissionsStore`; the roles screen and the guide say the same
   * thing where somebody grants these.
   */
  const canLifecycle = useCanOnServer(application?.serverId, "applications.lifecycle");
  const canConfigure = useCanOnServer(application?.serverId, "applications.config");

  /** After an action the Node has already carried out - the next read is
   * the authoritative one. */
  const reload = useCallback(() => {
    if (!id) return;
    void queryClient.invalidateQueries({ queryKey: queryKeys.application(id) });
  }, [id, queryClient]);

  /**
   * One chart sample per reading that arrives, not per render.
   *
   * Keyed on `dataUpdatedAt` rather than on the data: two identical
   * readings in a row are two samples, and a re-render that fetched nothing
   * is none.
   */
  useEffect(() => {
    const usage = usageQuery.data;
    if (!usage) return;
    setHistory((previous) => {
      const next = [...previous, { cpu: usage.cpuPercent ?? 0, ram: usage.ramBytes ?? 0 }];
      // Five minutes at the page's own refresh interval. Long enough to show
      // a spike settling, short enough that the window is about now rather
      // than about the whole session.
      return next.length > HISTORY_SAMPLES ? next.slice(next.length - HISTORY_SAMPLES) : next;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [usageQuery.dataUpdatedAt]);

  useEffect(() => {
    listBlueprints()
      .then((all) => {
        const found = all.find((b) => b.id === application?.blueprintId) ?? null;
        setBlueprint(found ? translateBlueprint(found, i18n.language) : null);
      })
      .catch(() => {});
  }, [application?.blueprintId, i18n.language]);


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

  /** Jumps the log pane to one end. Instant rather than smooth: over 500
   *  lines a smooth scroll is a long animation to sit through, and the point
   *  of the button is to be already there. */
  function scrollLogsTo(edge: "top" | "bottom") {
    const pane = logsRef.current;
    if (!pane) return;
    pane.scrollTop = edge === "top" ? 0 : pane.scrollHeight;
  }

  async function handleClearLogs() {
    if (!id) return;
    setClearLogsBusy(true);
    setLogsError(null);
    try {
      const archived = await clearApplicationLogs(id);
      setClearingLogs(false);
      // Where the copy went is the whole reason this is safe to press, so it
      // is said out loud rather than left in a directory nobody knows about.
      toastSuccess(archived ? t("applicationDetail.logsClearedTo", { path: archived }) : t("applicationDetail.logsAlreadyEmpty"));
      loadLogs();
    } catch (err) {
      setLogsError(errorMessage(err, t));
      setClearingLogs(false);
    } finally {
      setClearLogsBusy(false);
    }
  }

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
  // Two different questions, deliberately combined here rather than
  // conflated: whether the application's current state allows the verb, and
  // whether this member has been given the verb at all.
  const canStart = canLifecycle && (application ? ["stopped", "failed", "unknown"].includes(application.status) : false);
  const canStopOrRestart = canLifecycle && (application ? ["running", "starting"].includes(application.status) : false);
  const serverName = application?.serverId ? (servers.find((s) => s.id === application.serverId)?.name ?? application.serverId) : null;
  const features = blueprint?.features ?? [];
  /* Held steady across this page's five-second poll. Written inline it was a
     fresh array literal on every render, which is enough on its own to make
     the memoised rows inside the Files tab miss every time. */
  const knownFiles = useMemo(() => blueprint?.knownFiles ?? [], [blueprint]);
  const migrationTargets = servers.filter((s) => s.id !== application?.serverId);
  const migrateTargetWarning =
    migrateTargetServerId &&
    (servers.find((s) => s.id === migrateTargetServerId)?.connectionMode === "agent"
      ? servers.find((s) => s.id === migrateTargetServerId)?.capabilities?.docker === false
      : servers.find((s) => s.id === migrateTargetServerId)?.nodeCapabilities?.docker === false);

  return (
    <div className="page page-wide">
      {id && <ApplicationTabs activeId={id} />}
      <div className="page-header page-header-row">
        <div>
          <div className="application-detail-title-row">
            <h1 className="page-title">{application?.name ?? knownName ?? id}</h1>
            {application && (
              <IconButton
                icon="edit"
                size="sm"
                title={t("applicationDetail.renameAria", { name: application.name })}
                onClick={() => {
                  setRenameError(null);
                  setRenaming(application.name);
                }}
              />
            )}
          </div>
          {/* The blueprint's name, and next to it the step-by-step page for
              setting up this kind of application. Here rather than on a tab
              because "what is this and how do I configure it" is the
              question somebody has while looking at the whole page, and
              `GuideLink` shows nothing for a blueprint that has no topic
              written yet - so this appears per kind, as each is written. */}
          <p className="page-subtitle application-detail-kind">
            {blueprint ? blueprint.name : application?.blueprintId}
            {application && <GuideLink topic={`app-${application.blueprintId}`} />}
          </p>
        </div>
        <Button variant="secondary" onClick={() => navigate("/applications")}>
          <Icon name="chevron-left" size={16} />
          {t("applications.backToApplications")}
        </Button>
      </div>

      {renaming !== null && id && (
        <Dialog open onClose={() => setRenaming(null)} size="sm" dismissable={!renameBusy} title={t("applicationDetail.renameTitle")}>
          <form
            className="modal-body"
            onSubmit={async (event) => {
              event.preventDefault();
              if (renameBusy) return;
              setRenameBusy(true);
              setRenameError(null);
              try {
                await renameApplication(id, renaming);
                setRenaming(null);
                reload();
              } catch (err) {
                setRenameError(errorMessage(err, t));
              } finally {
                setRenameBusy(false);
              }
            }}
          >
            <label className="form-field">
              <span className="form-label">{t("applicationDetail.renameLabel")}</span>
              <input
                className="form-input"
                value={renaming}
                onChange={(event) => setRenaming(event.target.value)}
                maxLength={60}
                autoFocus
              />
            </label>
            {/* Worth saying here rather than in a doc comment nobody reads:
                this name is how linked Applications find this one. */}
            <p className="form-note">{t("applicationDetail.renameNote")}</p>
            {renameError && <p className="form-note form-note-danger form-note-spaced">{renameError}</p>}
            <div className="form-actions">
              <Button type="button" variant="secondary" onClick={() => setRenaming(null)} disabled={renameBusy}>
                {t("common.cancel")}
              </Button>
              <Button type="submit" disabled={renameBusy || renaming.trim().length === 0}>
                {t("common.save")}
              </Button>
            </div>
          </form>
        </Dialog>
      )}

      {loadError && <p className="page-error-note">{loadError}</p>}

      {application && (
        // Keyed by id so React remounts the body on a switch rather than
        // reconciling one Application's panels into another's - which is
        // what makes the fade read as a new page arriving instead of the
        // old one mutating in place.
        <div key={application.id} className="page-switch-fade">
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
              {application.runtimeType === "docker" && canConfigure && (
                <Button variant="secondary" size="sm" onClick={() => setConfirming("recreate")}>
                  <BlueprintIcon blueprintId={application?.blueprintId} size={14} />
                  {t("applicationDetail.verb.recreate")}
                </Button>
              )}
              {application.runtimeType === "docker" && migrationTargets.length > 0 && canConfigure && (
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
            {tab === "overview" && <TabUnderline group="application" />}
              </button>
            {/* Files sits second, right after the console.
                It is the tab an operator reaches for most once something is
                running - a config to edit, a plugin to drop in, a world to
                replace - and it was buried behind four tabs that are read
                far less often. The rest keep their existing relative order:
                only the one that was in the wrong place moved. */}
            {features.includes("files") && (
              <button className={`modal-tab ${tab === "files" ? "modal-tab-active" : ""}`} onClick={() => setTab("files")}>
                {t("applicationDetail.tabFiles")}
              {tab === "files" && <TabUnderline group="application" />}
              </button>
            )}
            {features.includes("logs") && (
              <button className={`modal-tab ${tab === "logs" ? "modal-tab-active" : ""}`} onClick={() => setTab("logs")}>
                {t("applicationDetail.tabLogs")}
              {tab === "logs" && <TabUnderline group="application" />}
              </button>
            )}
            {features.includes("ports") && (
              <button className={`modal-tab ${tab === "ports" ? "modal-tab-active" : ""}`} onClick={() => setTab("ports")}>
                {t("applicationDetail.tabPorts")}
              {tab === "ports" && <TabUnderline group="application" />}
              </button>
            )}
            {features.includes("databases") && (
              <button className={`modal-tab ${tab === "databases" ? "modal-tab-active" : ""}`} onClick={() => setTab("databases")}>
                {t("applicationDetail.tabDatabases")}
              {tab === "databases" && <TabUnderline group="application" />}
              </button>
            )}
            <button className={`modal-tab ${tab === "backups" ? "modal-tab-active" : ""}`} onClick={() => setTab("backups")}>
              {t("applicationDetail.tabBackups")}
            {tab === "backups" && <TabUnderline group="application" />}
              </button>
            <button className={`modal-tab ${tab === "settings" ? "modal-tab-active" : ""}`} onClick={() => setTab("settings")}>
              {t("applicationDetail.tabSettings")}
            {tab === "settings" && <TabUnderline group="application" />}
              </button>
          </div>

          {tab === "overview" && (
            <div className="application-detail-overview-grid">
              <div className="application-detail-overview">
                {/* First thing on a Paper/Purpur server's Overview: is it
                    healthy right now. Proxies (Velocity, Waterfall) have no
                    tick loop, and a local application has no tunnel to reach
                    RCON through, so neither gets the card. */}
                {(application.blueprintId === "paper" || application.blueprintId === "purpur") && application.serverId && (
                  <MinecraftMetricsCard applicationId={id} />
                )}

                {features.includes("console") && <ApplicationConsoleCard applicationId={id} isRunning={application.status === "running"} />}

                {/* Where the stdin console would be, for the kinds that have
                    no stdin console. A database ignores stdin, so it gets a
                    client instead - see `CommandConsoleCard`. Renders nothing
                    for a blueprint that declares none, which is most. */}
                <CommandConsoleCard applicationId={id} blueprint={blueprint} />

                {/* Under the console rather than beside it: these are worth
                    a glance, and the console is worth the width. Only while
                    there is something to plot - two empty boxes under a
                    stopped Application say less than nothing. */}
                {history.length > 0 && (
                  <div className="application-detail-charts">
                    <Card title={t("applicationDetail.cpu")}>
                      <p className="application-detail-chart-value">{resourceUsage?.cpuPercent?.toFixed(1) ?? "\u2014"}%</p>
                      <Sparkline
                        values={history.map((sample) => sample.cpu)}
                        label={t("applicationDetail.cpuChartLabel", { value: resourceUsage?.cpuPercent?.toFixed(1) ?? "0" })}
                      />
                    </Card>
                    <Card title={t("applicationDetail.ram")}>
                      <p className="application-detail-chart-value">
                        {resourceUsage?.ramBytes ? `${(resourceUsage.ramBytes / 1024 / 1024).toFixed(0)} MB` : "\u2014"}
                      </p>
                      <Sparkline
                        values={history.map((sample) => sample.ram / 1024 / 1024)}
                        label={t("applicationDetail.ramChartLabel", {
                          value: resourceUsage?.ramBytes ? (resourceUsage.ramBytes / 1024 / 1024).toFixed(0) : "0",
                        })}
                      />
                    </Card>
                  </div>
                )}
              </div>

              <aside className="application-detail-aside">
                <Card title={t("applicationDetail.resourceUsageTitle")}>
                  {application.status === "running" && resourceUsage ? (
                    <div className="application-detail-facts">
                      <div className="application-detail-fact">
                        <span className="form-label">{t("applicationDetail.cpu")}</span>
                        <p className="application-detail-stat-value">{resourceUsage.cpuPercent?.toFixed(1) ?? "\u2014"}%</p>
                      </div>
                      <div className="application-detail-fact">
                        <span className="form-label">{t("applicationDetail.ram")}</span>
                        <p className="application-detail-stat-value">
                          {resourceUsage.ramBytes ? `${(resourceUsage.ramBytes / 1024 / 1024).toFixed(0)} MB` : "\u2014"}
                        </p>
                      </div>
                      <div className="application-detail-fact">
                        <span className="form-label">{t("applicationDetail.uptime")}</span>
                        <p className="application-detail-stat-value">
                          {resourceUsage.uptimeSeconds ? formatUptime(resourceUsage.uptimeSeconds) : "\u2014"}
                        </p>
                      </div>
                    </div>
                  ) : (
                    <p className="form-note">{t("applicationDetail.notRunning")}</p>
                  )}
                </Card>

                <Card title={t("applicationDetail.whereTitle")}>
                  <div className="application-detail-facts">
                    <div className="application-detail-fact">
                      <span className="form-label">{t("applicationDetail.node")}</span>
                      <p className="application-detail-fact-value">{serverName ?? t("applicationCard.local")}</p>
                    </div>
                    <div className="application-detail-fact">
                      <span className="form-label">{t("applicationDetail.runtime")}</span>
                      <p className="application-detail-fact-value">{application.runtimeType}</p>
                    </div>
                    <div className="application-detail-fact">
                      <span className="form-label">{t("applicationDetail.workingDirectory")}</span>
                      <p className="application-detail-fact-value">{application.workingDirectory}</p>
                    </div>
                    {/* The last fact the removed Details card carried that
                        this column did not. Moved rather than kept as a
                        reason for a whole duplicate card to survive. */}
                    <div className="application-detail-fact">
                      <span className="form-label">{t("applicationDetail.createdAt")}</span>
                      <p className="application-detail-fact-value">{new Date(application.createdAt).toLocaleString()}</p>
                    </div>
                  </div>
                </Card>

                {features.includes("ports") && (
                  <Card title={t("applicationDetail.tabPorts")}>
                    {application.ports.length === 0 ? (
                      <p className="form-note">{t("applicationDetail.noPorts")}</p>
                    ) : (
                      <div className="application-detail-facts">
                        {application.ports.map((port) => (
                          <div key={port.id} className="application-detail-port">
                            <span className="application-detail-port-name">{port.name}</span>
                            <span className="application-detail-port-map">
                              {port.externalPort ? `${port.externalPort} \u2192 ${port.internalPort}` : `\u2014 \u2192 ${port.internalPort}`}
                              {" "}
                              {port.protocol}
                            </span>
                          </div>
                        ))}
                      </div>
                    )}
                  </Card>
                )}
              </aside>
            </div>
          )}

          {tab === "settings" && (
            <div className="application-detail-overview">

              <ApplicationConfigCard applicationId={id} application={application} blueprint={blueprint} onSaved={reload} />

              {/* Directly under the fields it changes the meaning of: this is
                  what decides whether the card above asks for a Paper version
                  or a container image. */}
              <BlueprintSwitchCard applicationId={id} application={application} current={blueprint} onChanged={reload} />

              {/* Folded in from its own tab. Environment variables and the
                  blueprint's own fields answer one question between them -
                  how this process comes up - and splitting that across two
                  tabs meant opening both to know the answer. Placed
                  directly after the blueprint fields and before the health
                  check and limits, which watch the result rather than
                  decide it. */}
              {features.includes("environment") && <EnvironmentTab application={application} blueprint={blueprint} onSaved={reload} />}

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
              <pre className="container-logs-output" ref={logsRef}>
                {logsLoading ? t("applicationDetail.logsLoading") : logs.join("\n") || t("applicationDetail.logsEmpty")}
              </pre>
              <div className="form-actions">
                <Button variant="secondary" onClick={() => scrollLogsTo("top")} disabled={logsLoading || logs.length === 0}>
                  <Icon name="chevron-up" size={14} />
                  {t("applicationDetail.logsToTop")}
                </Button>
                <Button variant="secondary" onClick={() => scrollLogsTo("bottom")} disabled={logsLoading || logs.length === 0}>
                  <Icon name="chevron-down" size={14} />
                  {t("applicationDetail.logsToBottom")}
                </Button>
                <Button variant="secondary" onClick={() => setClearingLogs(true)} disabled={logsLoading}>
                  <Icon name="trash" size={14} />
                  {t("applicationDetail.logsClear")}
                </Button>
                <Button variant="secondary" onClick={loadLogs} disabled={logsLoading}>
                  <Icon name="refresh-cw" size={14} />
                  {t("common.refresh")}
                </Button>
              </div>
            </Card>
          )}


          {tab === "ports" && <PortsTab applicationId={id} application={application} />}

          {tab === "databases" && <DatabasesTab applicationId={id} />}

          {tab === "files" && <ApplicationFilesTab applicationId={id} application={application} knownFiles={knownFiles} />}
          {tab === "backups" && <ApplicationBackupsTab applicationId={id} applicationStatus={application.status} />}
        </div>
      )}

      {clearingLogs && (
        <div className="modal-backdrop" {...clearLogsBackdrop.backdropProps}>
          <div className="modal-panel modal-panel-sm" {...clearLogsBackdrop.panelProps}>
            <div className="modal-header">
              <h2 className="modal-title" id="applicationdetail-dialog-title-3">{t("applicationDetail.logsClearTitle")}</h2>
              <IconButton icon="x" size="sm" onClick={() => setClearingLogs(false)} title={t("common.close")} />
            </div>
            <div className="modal-body">
              <p className="dialog-body-text">{t("applicationDetail.logsClearBody")}</p>
              <div className="form-actions">
                <Button variant="secondary" onClick={() => setClearingLogs(false)} disabled={clearLogsBusy}>
                  {t("common.cancel")}
                </Button>
                <Button variant="primary" onClick={handleClearLogs} disabled={clearLogsBusy}>
                  {clearLogsBusy ? t("applicationDetail.logsClearing") : t("applicationDetail.logsClear")}
                </Button>
              </div>
            </div>
          </div>
        </div>
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
