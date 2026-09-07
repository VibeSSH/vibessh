import { TabUnderline } from "@/components/ui/TabUnderline";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { POLL_INTERVALS, usePolling } from "@/hooks/usePolling";
import { STATUS_TONE } from "@/components/applications/ApplicationCard";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { NodeMiniCard } from "@/components/dashboard/NodeMiniCard";
import { MetricsHistoryChart } from "@/components/servers/MetricsHistoryChart";
import { TerminalView } from "@/components/servers/TerminalView";
import { useServerPinging } from "@/hooks/useServerPinging";
import { useServerMetricsPolling } from "@/hooks/useServerMetricsPolling";
import { listApplications } from "@/services/applicationService";
import { listNetworkMembers, getVibeNetworkStatus, syncVibeNetwork } from "@/services/networkService";
import { getNodeSyncStatus, listServers, reconcileAgentNode, serverSummaryToManagedServer, type NodeSyncStatus } from "@/services/serverService";
import { useServerModalStore } from "@/stores/serverModalStore";
import { useServersStore } from "@/stores/serversStore";
import { toastError, toastSuccess } from "@/stores/toastStore";
import type { Application } from "@/types/application";
import type { NodeMeshStatus, NodeNetworkMember } from "@/types/network";
import "./pages.css";
import "./Servers.css";
import "./Monitor.css";
import "./Dashboard.css";
import { errorMessage } from "@/services/tauri";
import { BlueprintIcon } from "@/components/applications/BlueprintIcon";


type WorkspaceTab = "applications" | "terminal" | "activity";

function formatPercent(value: number): string {
  return `${value.toFixed(0)}%`;
}

export function Dashboard() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const servers = useServersStore((s) => s.servers);
  const setServers = useServersStore((s) => s.setServers);
  const openForCreate = useServerModalStore((s) => s.openForCreate);

  const [applications, setApplications] = useState<Application[]>([]);
  const [networkMembers, setNetworkMembers] = useState<NodeNetworkMember[]>([]);
  const [meshStatus, setMeshStatus] = useState<NodeMeshStatus[]>([]);
  const [agentSync, setAgentSync] = useState<Record<string, NodeSyncStatus>>({});
  const [syncingNetwork, setSyncingNetwork] = useState(false);
  const [reconcilingId, setReconcilingId] = useState<string | null>(null);

  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<WorkspaceTab>("applications");
  const [alertsExpanded, setAlertsExpanded] = useState(false);
  const [tasksExpanded, setTasksExpanded] = useState(false);

  useServerPinging(servers);

  const sshServerIds = useMemo(() => servers.filter((s) => s.connectionMode === "ssh").map((s) => s.id), [servers]);
  const agentServerIds = useMemo(() => servers.filter((s) => s.connectionMode === "agent").map((s) => s.id), [servers]);
  const metricsByServer = useServerMetricsPolling(sshServerIds);

  useEffect(() => {
    listServers()
      .then((loaded) => setServers(loaded.map(serverSummaryToManagedServer)))
      .catch(() => {
        // No persisted servers yet, or this loaded outside a Tauri webview
        // during development - an empty list is the right fallback either way.
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Selecting a Node card only changes which context the workspace panel
  // below shows - if that Node disappears (removed, or never loaded), fall
  // back to the unscoped "all Nodes" view rather than pointing at nothing.
  useEffect(() => {
    if (selectedNodeId && !servers.some((s) => s.id === selectedNodeId)) {
      setSelectedNodeId(null);
    }
  }, [servers, selectedNodeId]);

  const reloadOverview = useCallback(() => {
    Promise.all([listApplications(), listNetworkMembers(), getVibeNetworkStatus()])
      .then(([apps, members, mesh]) => {
        setApplications(apps);
        setNetworkMembers(members);
        setMeshStatus(mesh);
      })
      .catch(() => {
        // Same fallback as the server list above - no Applications/Vibe
        // Network data yet is an honest empty overview, not an error banner.
      });
  }, []);

  usePolling(reloadOverview, POLL_INTERVALS.dashboardOverview);

  const agentKey = agentServerIds.join(",");
  // Keyed on the joined id list rather than the array itself, which is a new
  // reference on every render.
  const pollAgentSync = useCallback(async () => {
    const ids = agentKey ? agentKey.split(",") : [];
    const entries = await Promise.all(
      ids.map(async (id) => {
        try {
          return [id, await getNodeSyncStatus(id)] as const;
        } catch {
          return [id, null] as const;
        }
      }),
    );
    setAgentSync(Object.fromEntries(entries.filter((entry): entry is [string, NodeSyncStatus] => entry[1] !== null)));
  }, [agentKey]);

  usePolling(pollAgentSync, POLL_INTERVALS.dashboardOverview, { enabled: agentServerIds.length > 0 });

  function serverName(id: string): string {
    return servers.find((s) => s.id === id)?.name ?? id;
  }

  async function handleReconcile(id: string) {
    setReconcilingId(id);
    try {
      const outcome = await reconcileAgentNode(id);
      if (outcome.status === "failed") {
        toastError(outcome.error ?? t("serverCard.reconcileFailed"));
      } else {
        toastSuccess(t("dashboard.taskReconcileSuccess", { name: serverName(id) }));
      }
    } catch (err) {
      toastError(errorMessage(err, t));
    } finally {
      setReconcilingId(null);
      getNodeSyncStatus(id)
        .then((status) => setAgentSync((prev) => ({ ...prev, [id]: status })))
        .catch(() => {});
    }
  }

  async function handleSyncNetwork() {
    setSyncingNetwork(true);
    try {
      await syncVibeNetwork();
      toastSuccess(t("dashboard.quickSyncSuccess"));
      reloadOverview();
    } catch (err) {
      toastError(errorMessage(err, t));
    } finally {
      setSyncingNetwork(false);
    }
  }

  const online = servers.filter((s) => s.status === "online").length;

  const failedApps = applications.filter((a) => a.status === "failed");
  const filteredApplications = selectedNodeId ? applications.filter((a) => a.serverId === selectedNodeId) : applications;

  const networkHealthy = meshStatus.length > 0 && meshStatus.every((s) => s.reachable);
  const offlineServerIds = new Set(servers.filter((s) => s.status === "offline").map((s) => s.id));
  const unreachableMembers = meshStatus.filter((s) => !s.reachable && !offlineServerIds.has(s.serverId));
  const outOfSyncAgentIds = agentServerIds.filter((id) => agentSync[id] && !agentSync[id].inSync);

  interface AlertItem {
    id: string;
    icon: string;
    text: string;
    onClick: () => void;
  }
  const alerts: AlertItem[] = [
    ...servers
      .filter((s) => s.status === "offline")
      .map((s) => ({
        id: `server-${s.id}`,
        icon: "server",
        text: t("dashboard.alertServerOffline", { name: s.name }),
        onClick: () => setSelectedNodeId(s.id),
      })),
    ...unreachableMembers.map((m) => ({
      id: `mesh-${m.serverId}`,
      icon: "wifi",
      text: t("dashboard.alertNodeUnreachable", { name: serverName(m.serverId) }),
      onClick: () => navigate("/vibe-network"),
    })),
    ...failedApps.map((a) => ({
      id: `app-${a.id}`,
      icon: "box",
      text: t("dashboard.alertApplicationFailed", { name: a.name }),
      onClick: () => navigate(`/applications/${a.id}`),
    })),
  ];

  const anyProblem = alerts.length > 0 || outOfSyncAgentIds.length > 0;

  const applicationServerName = (app: Application) => (app.serverId ? serverName(app.serverId) : t("applicationCard.local"));

  const sshMetricsWithData = sshServerIds.map((id) => metricsByServer[id]?.latest).filter((m): m is NonNullable<typeof m> => !!m);
  const avgCpu = sshMetricsWithData.length > 0 ? sshMetricsWithData.reduce((sum, m) => sum + m.cpuUsagePercent, 0) / sshMetricsWithData.length : null;
  const avgRam =
    sshMetricsWithData.length > 0
      ? sshMetricsWithData.reduce((sum, m) => sum + (m.ramTotalBytes > 0 ? (m.ramUsedBytes / m.ramTotalBytes) * 100 : 0), 0) / sshMetricsWithData.length
      : null;
  const avgDisk =
    sshMetricsWithData.length > 0
      ? sshMetricsWithData.reduce((sum, m) => sum + (m.diskTotalBytes > 0 ? (m.diskUsedBytes / m.diskTotalBytes) * 100 : 0), 0) / sshMetricsWithData.length
      : null;

  const selectedServer = servers.find((s) => s.id === selectedNodeId) ?? null;
  const selectedMetricsState = selectedNodeId ? metricsByServer[selectedNodeId] : undefined;
  const selectedSyncStatus = selectedNodeId ? (agentSync[selectedNodeId] ?? null) : null;

  return (
    <div className="page dashboard-page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">{t("dashboard.title")}</h1>
          <p className="page-subtitle">{t("dashboard.subtitle")}</p>
        </div>
        {servers.length > 0 && (
          <div className="dashboard-header-actions">
            <Badge tone={anyProblem ? "danger" : "success"}>{anyProblem ? t("dashboard.overallProblem") : t("dashboard.overallHealthy")}</Badge>
            <IconButton icon="plug" title={t("servers.addServer")} onClick={openForCreate} />
            <IconButton icon="box" title={t("applications.create")} onClick={() => navigate("/applications")} />
            <IconButton icon="wifi" title={t("vibeNetwork.addNode")} onClick={() => navigate("/vibe-network")} />
            <IconButton
              icon="refresh-cw"
              title={syncingNetwork ? t("vibeNetwork.syncing") : t("vibeNetwork.syncButton")}
              onClick={handleSyncNetwork}
              disabled={syncingNetwork || networkMembers.length === 0}
            />
          </div>
        )}
      </div>

      {servers.length === 0 ? (
        <Card>
          <EmptyState
            icon="server"
            title={t("dashboard.emptyTitle")}
            description={t("dashboard.emptyDescription")}
            action={
              <Button onClick={openForCreate}>
                <Icon name="plug" size={16} />
                {t("servers.addServer")}
              </Button>
            }
          />
        </Card>
      ) : (
        <>
          <div className="dashboard-section">
            <h2 className="dashboard-section-title">{t("dashboard.sectionNodes", { count: servers.length })}</h2>
            <div className="dashboard-node-row">
              {servers.map((server) => (
                <NodeMiniCard
                  key={server.id}
                  server={server}
                  metrics={metricsByServer[server.id]?.latest ?? null}
                  history={metricsByServer[server.id]?.history ?? []}
                  syncStatus={agentSync[server.id] ?? null}
                  selected={server.id === selectedNodeId}
                  onSelect={() => setSelectedNodeId((prev) => (prev === server.id ? null : server.id))}
                />
              ))}
            </div>
          </div>

          <div className="dashboard-workspace">
            <div className="dashboard-workspace-header">
              <div className="page-tabs">
                <button type="button" className={`modal-tab ${activeTab === "applications" ? "modal-tab-active" : ""}`} onClick={() => setActiveTab("applications")}>
                  {t("dashboard.tabApplications")}
                {activeTab === "applications" && <TabUnderline group="dashboard" />}
                </button>
                <button type="button" className={`modal-tab ${activeTab === "terminal" ? "modal-tab-active" : ""}`} onClick={() => setActiveTab("terminal")}>
                  {t("dashboard.tabTerminal")}
                {activeTab === "terminal" && <TabUnderline group="dashboard" />}
                </button>
                <button type="button" className={`modal-tab ${activeTab === "activity" ? "modal-tab-active" : ""}`} onClick={() => setActiveTab("activity")}>
                  {t("dashboard.tabActivity")}
                {activeTab === "activity" && <TabUnderline group="dashboard" />}
                </button>
              </div>
              {selectedServer && (
                <div className="dashboard-workspace-selection">
                  <Icon name={selectedServer.connectionMode === "agent" ? "zap" : "server"} size={12} />
                  {selectedServer.name}
                  <button type="button" className="dashboard-workspace-clear" onClick={() => setSelectedNodeId(null)}>
                    {t("dashboard.clearSelection")}
                  </button>
                </div>
              )}
            </div>
            <div className="dashboard-workspace-divider" />

            <div className="dashboard-workspace-body">
              {activeTab === "applications" &&
                (filteredApplications.length === 0 ? (
                  <p className="dashboard-empty-row">{selectedServer ? t("dashboard.applicationsEmptyForNode") : t("dashboard.applicationsEmpty")}</p>
                ) : (
                  <ul className="server-list">
                    {filteredApplications.map((app) => (
                      <li key={app.id} className="server-list-item">
                        <div className="server-list-icon">
                          <BlueprintIcon blueprintId={app.blueprintId} size={16} />
                        </div>
                        <button type="button" className="server-list-main dashboard-row-btn" onClick={() => navigate(`/applications/${app.id}`)}>
                          <span className="server-list-name">{app.name}</span>
                          <span className="server-list-host">{applicationServerName(app)}</span>
                        </button>
                        <Badge tone={STATUS_TONE[app.status]}>{t(`applicationStatus.${app.status}`)}</Badge>
                      </li>
                    ))}
                  </ul>
                ))}

              {activeTab === "terminal" && (
                <div className="dashboard-workspace-terminal">
                  {!selectedServer ? (
                    <div className="dashboard-workspace-prompt">{t("dashboard.terminalSelectPrompt")}</div>
                  ) : selectedServer.connectionMode === "agent" ? (
                    <div className="dashboard-workspace-prompt">{t("dashboard.terminalAgentUnavailable")}</div>
                  ) : (
                    <TerminalView key={selectedServer.id} serverId={selectedServer.id} />
                  )}
                </div>
              )}

              {activeTab === "activity" &&
                (!selectedServer ? (
                  <div className="dashboard-workspace-prompt">{t("dashboard.activitySelectPrompt")}</div>
                ) : selectedServer.connectionMode === "agent" ? (
                  <div className="dashboard-workspace-activity-agent">
                    <p className="dashboard-empty-row">{t("dashboard.activityAgentNote")}</p>
                    {selectedSyncStatus && (
                      <span className={`node-mini-card-sync-pill ${selectedSyncStatus.inSync ? "node-mini-card-sync-pill-ok" : "node-mini-card-sync-pill-stale"}`}>
                        {selectedSyncStatus.inSync ? t("dashboard.nodeInSync") : t("dashboard.nodeOutOfSync")}
                      </span>
                    )}
                    {selectedSyncStatus && !selectedSyncStatus.inSync && (
                      <Button variant="secondary" size="sm" onClick={() => handleReconcile(selectedServer.id)} disabled={reconcilingId === selectedServer.id}>
                        <Icon name="refresh-cw" size={14} />
                        {t("dashboard.taskReconcileAction")}
                      </Button>
                    )}
                  </div>
                ) : selectedMetricsState?.latest ? (
                  <>
                    <div className="monitor-history-grid">
                      <MetricsHistoryChart
                        label={t("metricsPreview.cpu")}
                        values={selectedMetricsState.history.map((m) => m.cpuUsagePercent)}
                        formatValue={formatPercent}
                        minScale={100}
                      />
                      <MetricsHistoryChart
                        label={t("metricsPreview.ram")}
                        values={selectedMetricsState.history.map((m) => (m.ramTotalBytes > 0 ? (m.ramUsedBytes / m.ramTotalBytes) * 100 : 0))}
                        formatValue={formatPercent}
                        minScale={100}
                      />
                    </div>
                    <button type="button" className="dashboard-panel-link" onClick={() => navigate(`/monitor/${selectedServer.id}`)} style={{ marginTop: 12 }}>
                      {t("dashboard.openFullMonitor")}
                      <Icon name="chevron-right" size={12} />
                    </button>
                  </>
                ) : (
                  <div className="dashboard-workspace-prompt">{t("dashboard.nodeCollecting")}</div>
                ))}
            </div>
          </div>

          <Card>
            {/* Buttons, not clickable divs: these three were reachable with a
                mouse and with nothing else - no tab stop, no Enter, and
                nothing announcing that a row was a control at all. */}
            <button type="button" className="dashboard-ops-row" onClick={() => navigate("/vibe-network")}>
              <span className="dashboard-ops-label">
                <Icon name="wifi" size={14} />
                {t("vibeNetwork.title")}
              </span>
              <Badge tone={networkMembers.length === 0 ? "neutral" : networkHealthy ? "success" : "danger"}>
                {networkMembers.length === 0 ? t("dashboard.statNetworkValueNone") : networkHealthy ? t("vibeNetwork.networkHealthy") : t("vibeNetwork.networkDegraded")}
              </Badge>
            </button>

            <button
              type="button"
              className="dashboard-ops-row"
              onClick={() => setAlertsExpanded((v) => !v)}
              aria-expanded={alertsExpanded}
            >
              <span className="dashboard-ops-label">
                <Icon name="alert-triangle" size={14} />
                {t("dashboard.sectionAlerts")}
              </span>
              <Badge tone={alerts.length > 0 ? "danger" : "success"}>{alerts.length}</Badge>
            </button>
            {alertsExpanded &&
              (alerts.length === 0 ? (
                <p className="dashboard-empty-row dashboard-empty-row-ok dashboard-ops-expanded">
                  <span className="dashboard-empty-row-icon">
                    <Icon name="check" size={12} />
                  </span>
                  {t("dashboard.alertsEmpty")}
                </p>
              ) : (
                <ul className="server-list dashboard-ops-expanded">
                  {alerts.map((alert) => (
                    <li key={alert.id} className="server-list-item">
                      <div className="server-list-icon">
                        <Icon name={alert.icon} size={16} />
                      </div>
                      <button type="button" className="server-list-main dashboard-row-btn" onClick={alert.onClick}>
                        <span className="server-list-name">{alert.text}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              ))}

            {/* Only shown when there is something that could ever be out of
                sync. `reconcile` is Agent-mode only - it rejects SSH Nodes
                outright - so on an SSH-only install this section can never
                report anything but zero, and a permanently empty panel reads
                as a feature that is broken rather than one that does not
                apply. The sync poll is already disabled on the same
                condition. */}
            {agentServerIds.length > 0 && (
              <>
                <button type="button" className="dashboard-ops-row" onClick={() => setTasksExpanded((v) => !v)} aria-expanded={tasksExpanded}>
                  <span className="dashboard-ops-label">
                    <Icon name="list-checks" size={14} />
                    {t("dashboard.sectionTasks")}
                  </span>
                  <Badge tone={outOfSyncAgentIds.length > 0 ? "warning" : "success"}>{outOfSyncAgentIds.length}</Badge>
                </button>
                {tasksExpanded &&
                  (outOfSyncAgentIds.length === 0 ? (
                    <p className="dashboard-empty-row dashboard-empty-row-ok dashboard-ops-expanded">
                      <span className="dashboard-empty-row-icon">
                        <Icon name="check" size={12} />
                      </span>
                      {t("dashboard.tasksEmpty")}
                    </p>
                  ) : (
                    <div className="dashboard-ops-expanded">
                      {outOfSyncAgentIds.map((id) => (
                        <div key={id} className="dashboard-task-row">
                          <span className="dashboard-task-label">{serverName(id)}</span>
                          <Button variant="secondary" size="sm" onClick={() => handleReconcile(id)} disabled={reconcilingId === id}>
                            <Icon name="refresh-cw" size={14} />
                            {t("dashboard.taskReconcileAction")}
                          </Button>
                        </div>
                      ))}
                    </div>
                  ))}
              </>
            )}
          </Card>

          <div className="dashboard-footer-bar">
            <span className="dashboard-footer-bar-item">
              <Icon name="activity" size={13} />
              {t("metricsPreview.cpu")} <span className="dashboard-footer-bar-value">{avgCpu !== null ? formatPercent(avgCpu) : "—"}</span>
            </span>
            <span className="dashboard-footer-bar-item">
              {t("metricsPreview.ram")} <span className="dashboard-footer-bar-value">{avgRam !== null ? formatPercent(avgRam) : "—"}</span>
            </span>
            <span className="dashboard-footer-bar-item">
              {t("metricsPreview.disk")} <span className="dashboard-footer-bar-value">{avgDisk !== null ? formatPercent(avgDisk) : "—"}</span>
            </span>
            <span className="dashboard-footer-bar-spacer" />
            <span className={`dashboard-footer-bar-online ${online < servers.length ? "dashboard-footer-bar-online-problem" : ""}`}>
              <Icon name={online < servers.length ? "wifi-off" : "wifi"} size={13} />
              {t("dashboard.statOnlineValue", { online, total: servers.length })}
            </span>
          </div>
        </>
      )}
    </div>
  );
}
