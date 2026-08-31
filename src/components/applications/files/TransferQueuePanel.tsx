import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Card } from "@/components/ui/Card";
import { IconButton } from "@/components/ui/IconButton";
import { cancelApplicationFileTransfer } from "@/services/applicationFilesService";
import { useFileTransferStore, type TransferItem } from "@/stores/fileTransferStore";
import "./ApplicationFiles.css";

/** design brief section 111 - a real transfer manager, not a fake one: no "Pause" (neither the local filesystem copy nor SFTP has a resumable protocol this app speaks), but Cancel actually aborts the in-flight backend task (see state::FileTransferManager), and Retry re-issues the exact same request. */
export function TransferQueuePanel() {
  const { t } = useTranslation();
  const transfers = useFileTransferStore((s) => s.transfers);
  const removeTransfer = useFileTransferStore((s) => s.removeTransfer);
  const clearCompleted = useFileTransferStore((s) => s.clearCompleted);

  if (transfers.length === 0) return null;

  const hasCompleted = transfers.some((t) => t.status !== "transferring" && t.status !== "waiting");

  return (
    <Card title={t("transferQueue.title")} className="application-files-transfer-queue">
      {hasCompleted && (
        <div className="form-actions">
          <button className="application-files-quick-button" onClick={clearCompleted}>
            {t("transferQueue.clearCompleted")}
          </button>
        </div>
      )}
      {transfers.map((item) => (
        <TransferRow key={item.id} item={item} onRemove={() => removeTransfer(item.id)} />
      ))}
    </Card>
  );
}

function TransferRow({ item, onRemove }: { item: TransferItem; onRemove: () => void }) {
  const { t } = useTranslation();
  const percent = item.total > 0 ? Math.min(100, Math.round((item.transferred / item.total) * 100)) : 0;

  async function handleCancel() {
    await cancelApplicationFileTransfer(item.id);
  }

  return (
    <div className="application-files-transfer-row">
      <div className="application-files-transfer-header">
        <span className="application-files-transfer-name" title={item.name}>
          {item.direction === "upload" ? "↑" : "↓"} {item.name}
        </span>
        <StatusBadge status={item.status} />
      </div>
      <div className="application-files-transfer-bar">
        <div
          className={`application-files-transfer-bar-fill ${item.status === "error" ? "application-files-transfer-bar-fill-error" : ""}`}
          style={{ width: `${item.status === "done" ? 100 : percent}%` }}
        />
      </div>
      <div className="application-files-transfer-detail">
        {item.status === "transferring" && (
          <>
            <span>{formatBytes(item.transferred)} / {formatBytes(item.total)}</span>
            <span>{percent}%</span>
            {item.speedBps > 0 && <span>{formatBytes(item.speedBps)}/s</span>}
            {item.speedBps > 0 && item.total > item.transferred && <span>{formatEta((item.total - item.transferred) / item.speedBps)}</span>}
          </>
        )}
        {item.status === "error" && <span title={item.error}>{item.error}</span>}
        <span style={{ flex: 1 }} />
        {item.status === "transferring" && (
          <button className="application-files-quick-button" onClick={handleCancel}>
            {t("transferQueue.cancel")}
          </button>
        )}
        {item.status === "error" && item.retry && (
          <button className="application-files-quick-button" onClick={item.retry}>
            {t("transferQueue.retry")}
          </button>
        )}
        {item.status !== "transferring" && <IconButton icon="x" size="sm" onClick={onRemove} title={t("common.close")} />}
      </div>
    </div>
  );
}

function StatusBadge({ status }: { status: TransferItem["status"] }) {
  const { t } = useTranslation();
  const tone = status === "done" ? "success" : status === "error" ? "danger" : status === "canceled" ? "neutral" : "warning";
  return <Badge tone={tone}>{t(`transferQueue.status.${status}`)}</Badge>;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function formatEta(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "";
  if (seconds < 60) return `${Math.ceil(seconds)}s`;
  if (seconds < 3600) return `${Math.ceil(seconds / 60)}m`;
  return `${Math.ceil(seconds / 3600)}h`;
}
