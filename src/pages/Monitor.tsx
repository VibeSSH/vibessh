import { useCallback, useEffect, useState } from "react";
import { Navigate, useNavigate, useParams } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { POLL_INTERVALS, usePolling } from "@/hooks/usePolling";

import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { HostAddress } from "@/components/ui/HostAddress";
import { Icon } from "@/components/ui/Icon";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { MetricsPreview } from "@/components/servers/MetricsPreview";
import { MetricsHistoryChart } from "@/components/servers/MetricsHistoryChart";
import { getServerMetrics, listServerProcesses } from "@/services/monitorService";
import { useServersStore } from "@/stores/serversStore";
import type { ProcessSummary, ServerMetrics } from "@/types/serverEvent";
import "./pages.css";
import "./Monitor.css";

/** 5 minutes of history at the poll interval above - long enough to see a trend, short enough to stay a lightweight in-memory array. */
const HISTORY_LENGTH = 60;

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(1)} ${units[unitIndex]}`;
}

function formatPercent(value: number): string {
  return `${value.toFixed(0)}%`;
}

function formatRate(bytesPerSec: number): string {
  return `${formatBytes(bytesPerSec)}/s`;
}

export function MonitorPage() {
  const { t } = useTranslation();
  const { serverId } = useParams<{ serverId: string }>();
  const navigate = useNavigate();
  const server = useServersStore((s) => s.servers.find((srv) => srv.id === serverId));

  const [metrics, setMetrics] = useState<ServerMetrics | null>(null);
  const [history, setHistory] = useState<ServerMetrics[]>([]);
  const [processes, setProcesses] = useState<ProcessSummary[]>([]);
  const [error, setError] = useState<string | null>(null);

  // Switching Node clears the chart so the new host's history doesn't
  // continue the previous one's line.
  useEffect(() => {
    setHistory([]);
  }, [serverId]);

  const poll = useCallback(async () => {
    if (!serverId) return;
    try {
      const [nextMetrics, nextProcesses] = await Promise.all([getServerMetrics(serverId), listServerProcesses(serverId)]);
      setMetrics(nextMetrics);
      setHistory((prev) => [...prev, nextMetrics].slice(-HISTORY_LENGTH));
      setProcesses([...nextProcesses].sort((a, b) => b.ramBytes - a.ramBytes));
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : t("monitorPage.couldntReach"));
    }
  }, [serverId, t]);

  usePolling(poll, POLL_INTERVALS.monitor, { enabled: Boolean(serverId) });

  if (!serverId) {
    return <Navigate to="/servers" replace />;
  }

  return (
    <div className="page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">{server ? server.name : t("nav.monitor")}</h1>
          <p className="page-subtitle">{server ? <HostAddress value={server.host} /> : serverId}</p>
        </div>
        <Button variant="secondary" onClick={() => navigate("/servers")}>
          <Icon name="chevron-left" size={16} />
          {t("common.backToServers")}
        </Button>
      </div>

      {error && <p className="page-error-note">{error}</p>}

      <Card title={t("monitorPage.resources")} subtitle={t("monitorPage.refreshesEvery", { seconds: POLL_INTERVALS.monitor / 1000 })}>
        {metrics ? <MetricsPreview metrics={metrics} /> : <SkeletonRows count={3} height={52} />}
      </Card>

      <Card
        title={t("monitorPage.history")}
        subtitle={
          history.length > 1
            ? t("monitorPage.lastMinutes", { minutes: Math.round((history.length * POLL_INTERVALS.monitor) / 1000 / 60) })
            : t("monitorPage.collecting")
        }
      >
        {history.length === 0 ? (
          <SkeletonRows count={2} height={70} />
        ) : (
          <div className="monitor-history-grid">
            <MetricsHistoryChart label={t("monitorPage.cpu")} values={history.map((m) => m.cpuUsagePercent)} formatValue={formatPercent} minScale={100} />
            <MetricsHistoryChart
              label={t("monitorPage.ram")}
              values={history.map((m) => (m.ramTotalBytes > 0 ? (m.ramUsedBytes / m.ramTotalBytes) * 100 : 0))}
              formatValue={formatPercent}
              minScale={100}
            />
            <MetricsHistoryChart label={t("monitorPage.networkIn")} values={history.map((m) => m.networkRxBytesPerSec)} formatValue={formatRate} />
            <MetricsHistoryChart label={t("monitorPage.networkOut")} values={history.map((m) => m.networkTxBytesPerSec)} formatValue={formatRate} />
          </div>
        )}
      </Card>

      <Card title={t("monitorPage.processes")} subtitle={t("monitorPage.sortedByMemory", { count: processes.length })}>
        {processes.length === 0 ? (
          <SkeletonRows count={6} height={28} />
        ) : (
          <div className="monitor-process-table-wrap">
            <table className="monitor-process-table">
              <thead>
                <tr>
                  <th>{t("monitorPage.pid")}</th>
                  <th>{t("monitorPage.user")}</th>
                  <th>{t("monitorPage.cpuColumn")}</th>
                  <th>{t("monitorPage.ramColumn")}</th>
                  <th>{t("monitorPage.command")}</th>
                </tr>
              </thead>
              <tbody>
                {processes.slice(0, 50).map((process) => (
                  <tr key={process.pid}>
                    <td>{process.pid}</td>
                    <td>{process.user}</td>
                    <td>{process.cpuPercent.toFixed(1)}%</td>
                    <td>{formatBytes(process.ramBytes)}</td>
                    <td className="monitor-process-command" title={process.command}>{process.command}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
    </div>
  );
}
