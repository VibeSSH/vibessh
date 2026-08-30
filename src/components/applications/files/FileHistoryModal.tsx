import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { EmptyState } from "@/components/ui/EmptyState";
import { useBackdropClose } from "@/hooks/useBackdropClose";
import { listApplicationFileHistory, restoreApplicationFileHistory } from "@/services/applicationFilesService";
import type { FileHistoryVersion } from "@/types/applicationFiles";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";

interface FileHistoryModalProps {
  applicationId: string;
  path: string;
  fileName: string;
  onClose: () => void;
  onRestored: () => void;
}

/** Backup-before-save version history (design brief section 115) - List + Restore only. Preview/Diff are explicitly deferred by the brief itself ("może być Pro feature później") - the backend keeps every version's full content, so nothing here blocks adding those later. */
export function FileHistoryModal({ applicationId, path, fileName, onClose, onRestored }: FileHistoryModalProps) {
  const { t } = useTranslation();
  const backdrop = useBackdropClose(onClose);
  const [versions, setVersions] = useState<FileHistoryVersion[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [restoringTimestamp, setRestoringTimestamp] = useState<string | null>(null);

  useEffect(() => {
    listApplicationFileHistory(applicationId, path)
      .then(setVersions)
      .catch((err) => setError(err instanceof Error ? err.message : t("fileHistory.loadError")))
      .finally(() => setLoading(false));
  }, [applicationId, path, t]);

  async function handleRestore(timestamp: string) {
    setRestoringTimestamp(timestamp);
    setError(null);
    try {
      await restoreApplicationFileHistory(applicationId, path, timestamp);
      onRestored();
    } catch (err) {
      setError(err instanceof Error ? err.message : t("fileHistory.restoreError"));
    } finally {
      setRestoringTimestamp(null);
    }
  }

  return (
    <div className="modal-backdrop" {...backdrop}>
      <div className="modal-panel modal-panel-sm" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2 className="modal-title">{t("fileHistory.title", { name: fileName })}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <div className="modal-body">
          {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
          {loading ? (
            <SkeletonRows />
          ) : versions.length === 0 ? (
            <EmptyState icon="history" title={t("fileHistory.emptyTitle")} description={t("fileHistory.emptyDescription")} />
          ) : (
            <ul className="server-list">
              {versions.map((version) => (
                <li key={version.timestamp} className="server-list-item">
                  <div className="server-list-icon">
                    <Icon name="history" size={16} />
                  </div>
                  <div className="server-list-main">
                    <span className="server-list-name">{formatTimestamp(version.timestamp)}</span>
                    <span className="server-list-host">{formatSize(version.size)}</span>
                  </div>
                  <Button variant="secondary" size="sm" onClick={() => handleRestore(version.timestamp)} disabled={restoringTimestamp !== null}>
                    {restoringTimestamp === version.timestamp ? t("common.loading") : t("fileHistory.restore")}
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}

function formatTimestamp(timestamp: string): string {
  // "%Y-%m-%dT%H-%M-%SZ" (dashes, not colons - filesystem-safe on Windows) -> a real Date for locale formatting.
  const match = timestamp.match(/^(\d{4})-(\d{2})-(\d{2})T(\d{2})-(\d{2})-(\d{2})Z$/);
  if (!match) return timestamp;
  const [, year, month, day, hour, minute, second] = match;
  const date = new Date(Date.UTC(Number(year), Number(month) - 1, Number(day), Number(hour), Number(minute), Number(second)));
  return date.toLocaleString();
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
