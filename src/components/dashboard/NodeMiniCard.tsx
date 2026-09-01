import { useTranslation } from "react-i18next";
import { HostAddress } from "@/components/ui/HostAddress";
import { Icon } from "@/components/ui/Icon";
import { usePingStore } from "@/stores/pingStore";
import type { NodeSyncStatus } from "@/services/serverService";
import type { ManagedServer } from "@/stores/serversStore";
import type { ServerMetrics } from "@/types/serverEvent";
import "./NodeMiniCard.css";
import { StatusDot } from "@/components/ui/StatusDot";

function formatUptime(seconds: number): string {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  return `${minutes}m`;
}

const SPARK_WIDTH = 100;
const SPARK_HEIGHT = 22;

/** A bare trend line, no axis/label - MetricsHistoryChart's own visual recipe (line+area, same accent) shrunk down for a card this small. Renders nothing until there are at least 2 samples to draw a line between. */
function Sparkline({ values }: { values: number[] }) {
  if (values.length < 2) return <div className="node-mini-spark-empty" />;
  const max = Math.max(100, ...values);
  const points = values.map((value, index) => {
    const x = (index / (values.length - 1)) * SPARK_WIDTH;
    const y = SPARK_HEIGHT - (Math.max(0, value) / max) * SPARK_HEIGHT;
    return `${x.toFixed(2)},${y.toFixed(2)}`;
  });
  const line = points.join(" ");
  const area = `0,${SPARK_HEIGHT} ${line} ${SPARK_WIDTH},${SPARK_HEIGHT}`;
  return (
    <svg viewBox={`0 0 ${SPARK_WIDTH} ${SPARK_HEIGHT}`} preserveAspectRatio="none" className="node-mini-spark">
      <polygon points={area} className="node-mini-spark-area" />
      <polyline points={line} className="node-mini-spark-line" />
    </svg>
  );
}

function MiniBar({ label, percent }: { label: string; percent: number }) {
  const clamped = Math.min(100, Math.max(0, percent));
  return (
    <div className="node-mini-bar">
      <div className="node-mini-bar-header">
        <span>{label}</span>
        <span>{clamped.toFixed(0)}%</span>
      </div>
      <div className="node-mini-bar-track">
        <div className="node-mini-bar-fill" style={{ width: `${clamped}%` }} />
      </div>
    </div>
  );
}

interface NodeMiniCardProps {
  server: ManagedServer;
  metrics: ServerMetrics | null;
  history: ServerMetrics[];
  syncStatus: NodeSyncStatus | null;
  selected: boolean;
  onSelect: () => void;
}

/**
 * The Dashboard's own compact node summary - deliberately not ServerCard
 * (that card's glass surface, terminal-preview corner, and full action row
 * are sized for the Servers grid; this is a denser "one glance per Node"
 * unit meant to sit many-in-a-row). SSH-mode shows live CPU/RAM/uptime (the
 * data Monitor's own poll loop already proves reachable); Agent-mode has no
 * such metrics endpoint, so it shows its real sync state instead - the same
 * distinction ServerCard's action row already draws.
 */
export function NodeMiniCard({ server, metrics, history, syncStatus, selected, onSelect }: NodeMiniCardProps) {
  const { t } = useTranslation();
  const isAgent = server.connectionMode === "agent";
  const latencyMs = usePingStore((s) => s.latencies[server.id]);
  const ramPercent = metrics && metrics.ramTotalBytes > 0 ? (metrics.ramUsedBytes / metrics.ramTotalBytes) * 100 : 0;
  const needsAttention = server.status === "offline" || (isAgent && syncStatus !== null && !syncStatus.inSync);
  const cpuHistory = history.map((m) => m.cpuUsagePercent);

  return (
    // A `<div role="button">`, not a real `<button>` - the host address row
    // below needs its own real, nested `<button>` (the eye toggle), and a
    // `<button>` can't contain interactive content. onKeyDown restores the
    // Enter/Space activation a native button would have given for free.
    <div
      role="button"
      tabIndex={0}
      className={`node-mini-card ${needsAttention ? "node-mini-card-attention" : ""} ${selected ? "node-mini-card-selected" : ""}`}
      aria-pressed={selected}
      onClick={onSelect}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onSelect();
        }
      }}
    >
      <div className="node-mini-card-header">
        <div className={`node-mini-card-avatar glossy-tile ${isAgent ? "node-mini-card-avatar-agent" : ""}`}>
          {server.icon ? (
            <img src={server.icon} alt="" className="node-mini-card-icon-image" />
          ) : (
            <Icon name={isAgent ? "zap" : "server"} size={14} />
          )}
        </div>
        <div className="node-mini-card-title-col">
          <div className="node-mini-card-title-row">
            <span className="node-mini-card-name" title={server.name}>{server.name}</span>
            <span className="node-mini-card-protocol-pill">{isAgent ? "AGENT" : "SSH"}</span>
          </div>
          <HostAddress value={server.host} prefix={server.username ? `${server.username}@` : undefined} className="node-mini-card-host" />
        </div>
        <StatusDot status={server.status} withLabel />
      </div>

      {isAgent ? (
        <div className="node-mini-card-sync-row">
          {syncStatus ? (
            <span className={`node-mini-card-sync-pill ${syncStatus.inSync ? "node-mini-card-sync-pill-ok" : "node-mini-card-sync-pill-stale"}`}>
              {syncStatus.inSync ? t("dashboard.nodeInSync") : t("dashboard.nodeOutOfSync")}
            </span>
          ) : (
            <span className="node-mini-card-metric-empty">{t("dashboard.nodeStatusUnknown")}</span>
          )}
        </div>
      ) : metrics ? (
        <>
          <div className="node-mini-card-bars">
            <MiniBar label={t("monitorPage.cpu")} percent={metrics.cpuUsagePercent} />
            <MiniBar label={t("monitorPage.ram")} percent={ramPercent} />
          </div>
          <div className="node-mini-card-footer-row">
            <span title={t("dashboard.nodeUptimeAria")}>
              <Icon name="history" size={11} /> {formatUptime(metrics.uptimeSeconds)}
            </span>
            {typeof latencyMs === "number" && <span className="node-mini-card-latency">{latencyMs} ms</span>}
          </div>
          <Sparkline values={cpuHistory} />
        </>
      ) : (
        <p className="node-mini-card-metric-empty">{server.status === "offline" ? t("dashboard.nodeOffline") : t("dashboard.nodeCollecting")}</p>
      )}
    </div>
  );
}
