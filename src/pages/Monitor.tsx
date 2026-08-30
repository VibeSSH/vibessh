import { useEffect, useState } from "react";
import { Navigate, useNavigate, useParams } from "react-router-dom";

import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { MetricsPreview } from "@/components/servers/MetricsPreview";
import { getServerMetrics, listServerProcesses } from "@/services/monitorService";
import { useServersStore } from "@/stores/serversStore";
import type { ProcessSummary, ServerMetrics } from "@/types/serverEvent";
import "./pages.css";
import "./Monitor.css";

const POLL_INTERVAL_MS = 5000;

export function MonitorPage() {
  const { serverId } = useParams<{ serverId: string }>();
  const navigate = useNavigate();
  const server = useServersStore((s) => s.servers.find((srv) => srv.id === serverId));

  const [metrics, setMetrics] = useState<ServerMetrics | null>(null);
  const [processes, setProcesses] = useState<ProcessSummary[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!serverId) return;
    let cancelled = false;

    async function poll() {
      try {
        const [nextMetrics, nextProcesses] = await Promise.all([
          getServerMetrics(serverId!),
          listServerProcesses(serverId!),
        ]);
        if (cancelled) return;
        setMetrics(nextMetrics);
        setProcesses([...nextProcesses].sort((a, b) => b.ramBytes - a.ramBytes));
        setError(null);
      } catch (err) {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : "Couldn't reach this server.");
      }
    }

    poll();
    const id = window.setInterval(poll, POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [serverId]);

  if (!serverId) {
    return <Navigate to="/servers" replace />;
  }

  return (
    <div className="page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">{server ? server.name : "Monitor"}</h1>
          <p className="page-subtitle">{server ? server.host : serverId}</p>
        </div>
        <Button variant="secondary" onClick={() => navigate("/servers")}>
          <Icon name="chevron-left" size={16} />
          Back to servers
        </Button>
      </div>

      {error && <p className="page-error-note">{error}</p>}

      <Card title="Resources" subtitle={`Refreshes every ${POLL_INTERVAL_MS / 1000}s`}>
        {metrics ? <MetricsPreview metrics={metrics} /> : <SkeletonRows count={3} height={52} />}
      </Card>

      <Card title="Processes" subtitle={`${processes.length} running, sorted by memory`}>
        {processes.length === 0 ? (
          <SkeletonRows count={6} height={28} />
        ) : (
          <div className="monitor-process-table-wrap">
            <table className="monitor-process-table">
              <thead>
                <tr>
                  <th>PID</th>
                  <th>User</th>
                  <th>CPU</th>
                  <th>RAM</th>
                  <th>Command</th>
                </tr>
              </thead>
              <tbody>
                {processes.slice(0, 50).map((process) => (
                  <tr key={process.pid}>
                    <td>{process.pid}</td>
                    <td>{process.user}</td>
                    <td>{process.cpuPercent.toFixed(1)}%</td>
                    <td>{formatBytes(process.ramBytes)}</td>
                    <td className="monitor-process-command">{process.command}</td>
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

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(1)} ${units[unitIndex]}`;
}
