import { useTranslation } from "react-i18next";
import type { ServerMetrics } from "@/types/serverEvent";
import "./MetricsPreview.css";

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

function formatRate(bytesPerSec: number): string {
  return `${formatBytes(bytesPerSec)}/s`;
}

function formatUptime(seconds: number): string {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  return `${minutes}m`;
}

function Gauge({ label, percent }: { label: string; percent: number }) {
  const clamped = Math.min(100, Math.max(0, percent));
  return (
    <div className="metrics-gauge">
      <div className="metrics-gauge-header">
        <span>{label}</span>
        <span>{clamped.toFixed(0)}%</span>
      </div>
      <div className="metrics-gauge-track">
        <div className="metrics-gauge-fill" style={{ width: `${clamped}%` }} />
      </div>
    </div>
  );
}

interface MetricsPreviewProps {
  metrics: ServerMetrics;
}

/**
 * Etap J: a live preview fed by the WebSocket connection the pairing flow
 * already has open - real push data, not polled, not mocked. This is not
 * the full Dashboard (there's no persistent per-server session to feed one
 * yet, see the connection-lifetime note in AgentPairingFlow), just proof
 * the whole realtime pipeline actually works end to end in the UI.
 */
export function MetricsPreview({ metrics }: MetricsPreviewProps) {
  const { t } = useTranslation();
  const ramPercent = metrics.ramTotalBytes > 0 ? (metrics.ramUsedBytes / metrics.ramTotalBytes) * 100 : 0;
  const diskPercent = metrics.diskTotalBytes > 0 ? (metrics.diskUsedBytes / metrics.diskTotalBytes) * 100 : 0;

  return (
    <div className="metrics-preview">
      <Gauge label={t("metricsPreview.cpu")} percent={metrics.cpuUsagePercent} />
      <Gauge label={t("metricsPreview.ram")} percent={ramPercent} />
      <Gauge label={t("metricsPreview.disk")} percent={diskPercent} />
      <div className="metrics-preview-stats">
        <span>{t("metricsPreview.load1m", { value: metrics.loadAverage1m.toFixed(2) })}</span>
        <span>{t("metricsPreview.uptime", { value: formatUptime(metrics.uptimeSeconds) })}</span>
        <span>
          {t("metricsPreview.net", { down: formatRate(metrics.networkRxBytesPerSec), up: formatRate(metrics.networkTxBytesPerSec) })}
        </span>
      </div>
    </div>
  );
}
