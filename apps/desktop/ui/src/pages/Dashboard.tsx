import { TabUnderline } from "@/components/ui/TabUnderline";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import RGL, { WidthProvider, type Layout } from "react-grid-layout";
import "react-grid-layout/css/styles.css";
import "react-resizable/css/styles.css";
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
import { NodeIcon } from "@/components/servers/NodeIcon";
import { StatusDot } from "@/components/ui/StatusDot";
import { OverflowMenu } from "@/components/ui/OverflowMenu";
import { usePingStore } from "@/stores/pingStore";
// The compact server rows and the Activity tab's agent view reuse the sync
// pill styles that live in this stylesheet; keep it loaded now that the
// NodeMiniCard component itself is no longer rendered here.
import "@/components/dashboard/NodeMiniCard.css";
import { MetricsHistoryChart } from "@/components/servers/MetricsHistoryChart";
import { TerminalView } from "@/components/servers/TerminalView";
import { useServerPinging } from "@/hooks/useServerPinging";
import { useServerMetricsStream } from "@/hooks/useServerMetricsStream";
import { listApplications } from "@/services/applicationService";
import { listNetworkMembers, getVibeNetworkStatus, syncVibeNetwork } from "@/services/networkService";
import { getNodeSyncStatus, listServers, reconcileAgentNode, serverSummaryToManagedServer, type NodeSyncStatus } from "@/services/serverService";
import { useServerModalStore } from "@/stores/serverModalStore";
import { useServersStore } from "@/stores/serversStore";
import { useApplicationsStore } from "@/stores/applicationsStore";
import { useSelectedNodeStore } from "@/stores/selectedNodeStore";
import { toastError, toastSuccess } from "@/stores/toastStore";
import type { Application } from "@/types/application";
import type { NodeMeshStatus, NodeNetworkMember } from "@/types/network";
import "./pages.css";
import "./Servers.css";
import "./Monitor.css";
import "./Dashboard.css";
import { errorMessage } from "@/services/tauri";
import { formatBytes } from "@/utils/formatBytes";
import { BlueprintIcon } from "@/components/applications/BlueprintIcon";


const GridLayout = WidthProvider(RGL);

// v2: the grid row unit changed (finer rows for tighter auto-fit), which makes
// heights saved under the old unit meaningless - a new key retires them.
const LAYOUT_KEY = "vibessh_dashboard_layout_v2";
const ROW_HEIGHT = 12;
const GRID_MARGIN: [number, number] = [16, 12];

/** The full-width bands whose height is driven by their content (the metric
 *  tiles and the ops list) rather than by the user - measured and auto-fit so a
 *  fixed grid cell never clips them. They reorder by dragging but do not resize.
 */
const AUTOFIT_KEYS = new Set(["metrics", "ops"]);

/** The number of grid rows a band of `contentPx` natural height needs, so its
 *  cell fits the content exactly (never shorter - that is what clipped it). */
function rowsForHeight(contentPx: number, minH: number): number {
  return Math.max(minH, Math.ceil((contentPx + GRID_MARGIN[1]) / (ROW_HEIGHT + GRID_MARGIN[1])));
}

/** The default arrangement, and the source of truth for which widgets exist. */
const DEFAULT_LAYOUT: Layout[] = [
  { i: "metrics", x: 0, y: 0, w: 12, h: 7, minW: 4, minH: 5, isResizable: false },
  { i: "servers", x: 0, y: 7, w: 7, h: 14, minW: 4, minH: 8 },
  { i: "workspace", x: 7, y: 7, w: 5, h: 14, minW: 3, minH: 8 },
  { i: "ops", x: 0, y: 21, w: 12, h: 6, minW: 4, minH: 4, isResizable: false },
];

function loadLayout(): Layout[] {
  try {
    const raw = localStorage.getItem(LAYOUT_KEY);
    if (!raw) return DEFAULT_LAYOUT;
    const saved = JSON.parse(raw) as Layout[];
    // Keep the known widgets, filling in the default for any the saved layout
    // is missing - so a layout saved before a widget existed never hides it.
    const byId = new Map(saved.map((item) => [item.i, item]));
    return DEFAULT_LAYOUT.map((def) => byId.get(def.i) ?? def);
  } catch {
    return DEFAULT_LAYOUT;
  }
}

function saveLayout(layout: Layout[]) {
  try {
    localStorage.setItem(LAYOUT_KEY, JSON.stringify(layout));
  } catch {
    // Non-fatal - the arrangement just won't persist this session.
  }
}

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

  // Shared with the sidebar connection selector so both drive the same focus.
  const selectedNodeId = useSelectedNodeStore((s) => s.selectedNodeId);
  const setSelectedNodeId = useSelectedNodeStore((s) => s.setSelectedNodeId);
  // Landing here with a Node already picked (from the sidebar selector) opens
  // straight onto its live stats rather than the applications list.
  const [activeTab, setActiveTab] = useState<WorkspaceTab>(selectedNodeId ? "activity" : "applications");
  const [alertsExpanded, setAlertsExpanded] = useState(false);
  const [tasksExpanded, setTasksExpanded] = useState(false);
  // When the metric tiles last took a fresh reading, and a 1s tick that keeps
  // the "x s ago" label climbing between polls.
  const [metricsUpdatedAt, setMetricsUpdatedAt] = useState<number | null>(null);
  const [, setSecondsTick] = useState(0);
  // The customizable widget grid: an "edit layout" toggle unlocks drag/resize,
  // and the arrangement is remembered per device.
  const [editing, setEditing] = useState(false);
  const [layout, setLayout] = useState<Layout[]>(() => loadLayout());
  const handleLayoutChange = useCallback((next: Layout[]) => {
    setLayout(next);
    saveLayout(next);
  }, []);
  const resetLayout = useCallback(() => {
    setLayout(DEFAULT_LAYOUT);
    saveLayout(DEFAULT_LAYOUT);
  }, []);

  // The content-driven bands (metrics, ops) measure their own natural height and
  // grow/shrink their grid rows to fit it, so nothing is ever clipped - locale
  // text, more or fewer metric tiles, and the alerts/tasks sections expanding
  // all change the height, and a fixed cell would otherwise cut off the bottom.
  const autoFitRefs = useRef<Record<string, HTMLDivElement | null>>({});
  const layoutRef = useRef(layout);
  layoutRef.current = layout;
  useLayoutEffect(() => {
    const fit = () => {
      let changed = false;
      const next = layoutRef.current.map((item) => {
        if (!AUTOFIT_KEYS.has(item.i)) return item;
        const content = autoFitRefs.current[item.i]?.firstElementChild as HTMLElement | null;
        if (!content) return item;
        const needed = rowsForHeight(content.offsetHeight, item.minH ?? 1);
        if (needed !== item.h) {
          changed = true;
          return { ...item, h: needed };
        }
        return item;
      });
      if (changed) {
        setLayout(next);
        saveLayout(next);
      }
    };
    const observer = new ResizeObserver(fit);
    Object.values(autoFitRefs.current).forEach((host) => {
      const content = host?.firstElementChild;
      if (content) observer.observe(content);
    });
    fit();
    return () => observer.disconnect();
  }, []);

  useServerPinging(servers);
  const latencies = usePingStore((s) => s.latencies);

  const sshServerIds = useMemo(() => servers.filter((s) => s.connectionMode === "ssh").map((s) => s.id), [servers]);
  const agentServerIds = useMemo(() => servers.filter((s) => s.connectionMode === "agent").map((s) => s.id), [servers]);
  const metricsByServer = useServerMetricsStream(sshServerIds);

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

  // Focusing a Node - whether from the Servers panel below or the sidebar
  // connection selector - brings its live stats (the Activity tab) to the
  // front of the workspace rather than leaving the console there.
  useEffect(() => {
    if (selectedNodeId) setActiveTab("activity");
  }, [selectedNodeId]);

  const reloadOverview = useCallback(() => {
    Promise.all([listApplications(), listNetworkMembers(), getVibeNetworkStatus()])
      .then(([apps, members, mesh]) => {
        setApplications(apps);
        // Also into the shared store, so the sidebar's Applications count is
        // right from the landing page without a second fetch.
        useApplicationsStore.getState().setApplications(apps);
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

  // With a Node picked in the Servers panel the three tiles scope to that
  // Node's own figures; with nothing picked they show the fleet averages.
  const scopedMetric = selectedNodeId ? (metricsByServer[selectedNodeId]?.latest ?? null) : null;
  const cpuValue = scopedMetric ? scopedMetric.cpuUsagePercent : avgCpu;
  const ramValue = scopedMetric ? (scopedMetric.ramTotalBytes > 0 ? (scopedMetric.ramUsedBytes / scopedMetric.ramTotalBytes) * 100 : 0) : avgRam;
  const diskValue = scopedMetric ? (scopedMetric.diskTotalBytes > 0 ? (scopedMetric.diskUsedBytes / scopedMetric.diskTotalBytes) * 100 : 0) : avgDisk;

  // Footer data, replacing the bare Node count: CPU shows the load average,
  // RAM and Disk show used / total in real units - scoped to the picked Node,
  // or summed across the Nodes with data when none is picked.
  const avgLoad = sshMetricsWithData.length > 0 ? sshMetricsWithData.reduce((s, m) => s + m.loadAverage1m, 0) / sshMetricsWithData.length : null;
  const loadFoot = scopedMetric ? scopedMetric.loadAverage1m : avgLoad;
  const ramUsed = scopedMetric ? scopedMetric.ramUsedBytes : sshMetricsWithData.reduce((s, m) => s + m.ramUsedBytes, 0);
  const ramTotal = scopedMetric ? scopedMetric.ramTotalBytes : sshMetricsWithData.reduce((s, m) => s + m.ramTotalBytes, 0);
  const diskUsed = scopedMetric ? scopedMetric.diskUsedBytes : sshMetricsWithData.reduce((s, m) => s + m.diskUsedBytes, 0);
  const diskTotal = scopedMetric ? scopedMetric.diskTotalBytes : sshMetricsWithData.reduce((s, m) => s + m.diskTotalBytes, 0);
  const bytesFoot = (used: number, total: number) => (total > 0 ? `${formatBytes(used)} / ${formatBytes(total)}` : "-");

  // Picking a Node in the Servers panel jumps the workspace to its live stats
  // (the Activity tab) rather than leaving the console in front.
  const handleSelectNode = (id: string) => {
    setSelectedNodeId(selectedNodeId === id ? null : id);
  };

  // A poll replaces the whole metrics map, so its reference changing is the
  // signal that a fresh reading just landed. The 1s tick keeps the label
  // climbing between polls, which also makes a stalled poll visible - the
  // number simply keeps growing past the 6s interval.
  useEffect(() => {
    if (sshMetricsWithData.length > 0) setMetricsUpdatedAt(Date.now());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [metricsByServer]);
  useEffect(() => {
    const id = window.setInterval(() => setSecondsTick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, []);

  const metricsUpdatedLabel =
    metricsUpdatedAt == null
      ? null
      : (() => {
          const seconds = Math.max(0, Math.round((Date.now() - metricsUpdatedAt) / 1000));
          return seconds < 2 ? t("dashboard.updatedNow") : t("dashboard.updatedAgo", { s: seconds });
        })();

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
            {editing && (
              <button type="button" className="dashboard-edit-reset" onClick={resetLayout}>
                {t("dashboard.resetLayout")}
              </button>
            )}
            <IconButton
              icon="layout-grid"
              title={editing ? t("dashboard.lockLayout") : t("dashboard.editLayout")}
              onClick={() => setEditing((v) => !v)}
            />
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
        <GridLayout
          className={`dashboard-grid ${editing ? "dashboard-grid-editing" : ""}`}
          layout={layout}
          cols={12}
          rowHeight={ROW_HEIGHT}
          margin={GRID_MARGIN}
          isDraggable={editing}
          isResizable={editing}
          onLayoutChange={handleLayoutChange}
          compactType="vertical"
        >
          <div key="metrics" className="dash-widget" data-autofit ref={(el) => { autoFitRefs.current.metrics = el; }}>
          {/* Three-metric row (the reference's top band). With a Node picked in
              the Servers panel below, the tiles read that Node's own CPU / RAM /
              Disk and their footers its load and used/total; with nothing picked
              they show the fleet averages and summed totals. */}
          <div className="dashboard-metrics">
            <div className="stat-card">
              <div className="stat-card-top">
                <span className="stat-card-label">CPU</span>
                <Icon name="activity" size={15} className="stat-card-icon" />
              </div>
              <div className="stat-card-value">{cpuValue != null ? `${cpuValue.toFixed(1)}%` : "-"}</div>
              <div className="stat-card-bar">
                <span className="stat-card-bar-fill" style={{ transform: `scaleX(${Math.min(100, cpuValue ?? 0) / 100})` }} />
              </div>
              <div className="stat-card-foot">{loadFoot != null ? t("dashboard.loadFoot", { load: loadFoot.toFixed(2) }) : "-"}</div>
              {metricsUpdatedLabel && <div className="stat-card-updated">{metricsUpdatedLabel}</div>}
            </div>
            <div className="stat-card">
              <div className="stat-card-top">
                <span className="stat-card-label">{t("rail.ram")}</span>
                <Icon name="server" size={15} className="stat-card-icon" />
              </div>
              <div className="stat-card-value">{ramValue != null ? `${ramValue.toFixed(0)}%` : "-"}</div>
              <div className="stat-card-bar">
                <span className="stat-card-bar-fill" style={{ transform: `scaleX(${Math.min(100, ramValue ?? 0) / 100})` }} />
              </div>
              <div className="stat-card-foot">{bytesFoot(ramUsed, ramTotal)}</div>
              {metricsUpdatedLabel && <div className="stat-card-updated">{metricsUpdatedLabel}</div>}
            </div>
            <div className="stat-card">
              <div className="stat-card-top">
                <span className="stat-card-label">{t("rail.disk")}</span>
                <Icon name="database" size={15} className="stat-card-icon" />
              </div>
              <div className="stat-card-value">{diskValue != null ? `${diskValue.toFixed(0)}%` : "-"}</div>
              <div className="stat-card-bar">
                <span className="stat-card-bar-fill" style={{ transform: `scaleX(${Math.min(100, diskValue ?? 0) / 100})` }} />
              </div>
              <div className="stat-card-foot">{bytesFoot(diskUsed, diskTotal)}</div>
              {metricsUpdatedLabel && <div className="stat-card-updated">{metricsUpdatedLabel}</div>}
            </div>
          </div>
          </div>

          <div key="servers" className="dash-widget">
          <section className="dashboard-panel dashboard-servers-panel">
            <div className="dashboard-panel-head">
              <h2 className="dashboard-panel-title">{t("nav.servers")}</h2>
              <span className={`dashboard-panel-count ${online < servers.length ? "dashboard-panel-count-problem" : ""}`}>
                {t("dashboard.statOnlineValue", { online, total: servers.length })}
              </span>
            </div>
            <ul className="dashboard-server-rows">
              {servers.map((server) => {
                const isAgent = server.connectionMode === "agent";
                const m = metricsByServer[server.id]?.latest ?? null;
                const ram = m && m.ramTotalBytes > 0 ? (m.ramUsedBytes / m.ramTotalBytes) * 100 : null;
                const latency = latencies[server.id];
                const syncState = agentSync[server.id] ?? null;
                const selected = server.id === selectedNodeId;
                const rowActions = [
                  ...(isAgent ? [] : [{ label: t("nav.terminal"), icon: "terminal", onClick: () => navigate(`/terminal/${server.id}`) }]),
                  { label: t("nav.monitor"), icon: "activity", onClick: () => navigate(`/monitor/${server.id}`) },
                  ...(isAgent && syncState && !syncState.inSync
                    ? [{ label: t("dashboard.taskReconcileAction"), icon: "refresh-cw", onClick: () => handleReconcile(server.id) }]
                    : []),
                ];
                return (
                  <li key={server.id} className={`dashboard-server-row ${selected ? "dashboard-server-row-active" : ""}`}>
                    <button
                      type="button"
                      className="dashboard-server-row-main"
                      onClick={() => handleSelectNode(server.id)}
                      aria-pressed={selected}
                    >
                      <span className="dashboard-server-row-icon">
                        <NodeIcon server={server} size={15} />
                      </span>
                      <span className="dashboard-server-row-id">
                        <span className="dashboard-server-row-name" title={server.name}>
                          {server.name}
                        </span>
                        <StatusDot status={server.status} withLabel />
                      </span>
                      <span className="dashboard-server-row-stats">
                        {isAgent ? (
                          <span
                            className={`node-mini-card-sync-pill ${syncState?.inSync ? "node-mini-card-sync-pill-ok" : "node-mini-card-sync-pill-stale"}`}
                          >
                            {syncState ? (syncState.inSync ? t("dashboard.nodeInSync") : t("dashboard.nodeOutOfSync")) : t("dashboard.nodeStatusUnknown")}
                          </span>
                        ) : m ? (
                          <>
                            <span className="dashboard-server-stat">
                              <i>CPU</i>
                              {m.cpuUsagePercent.toFixed(0)}%
                            </span>
                            <span className="dashboard-server-stat">
                              <i>RAM</i>
                              {ram != null ? `${ram.toFixed(0)}%` : "-"}
                            </span>
                            <span className="dashboard-server-stat dashboard-server-stat-latency">{typeof latency === "number" ? `${latency} ms` : "-"}</span>
                          </>
                        ) : (
                          <span className="dashboard-server-stat-empty">
                            {server.status === "offline" ? t("dashboard.nodeOffline") : t("dashboard.nodeCollecting")}
                          </span>
                        )}
                      </span>
                    </button>
                    <OverflowMenu ariaLabel={server.name} items={rowActions} />
                  </li>
                );
              })}
            </ul>
          </section>
          </div>

          <div key="workspace" className="dash-widget">
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
                  // data-lenis-prevent: the app shell scrolls through Lenis,
                  // which swallows wheel events for smooth scrolling - without
                  // this the nested list shows a scrollbar but the wheel scrolls
                  // the page instead of the list.
                  <ul className="server-list" data-lenis-prevent>
                    {filteredApplications.map((app) => (
                      <li key={app.id} className="server-list-item">
                        <div className="server-list-icon">
                          <BlueprintIcon blueprintId={app.blueprintId} size={14} />
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
          </div>

          <div key="ops" className="dash-widget" data-autofit ref={(el) => { autoFitRefs.current.ops = el; }}>
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
          </div>
        </GridLayout>
      )}
    </div>
  );
}
