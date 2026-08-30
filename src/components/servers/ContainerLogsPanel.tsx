import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Icon } from "@/components/ui/Icon";
import { getServerContainerLogs } from "@/services/actionsService";
import "./AddServerModal.css";
import "./forms.css";

const TAIL_LINES = 500;

interface ContainerLogsPanelProps {
  serverId: string;
  containerName: string;
  onClose: () => void;
}

export function ContainerLogsPanel({ serverId, containerName, onClose }: ContainerLogsPanelProps) {
  const { t } = useTranslation();
  const [logs, setLogs] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    setLoading(true);
    setError(null);
    getServerContainerLogs(serverId, containerName, TAIL_LINES)
      .then(setLogs)
      .catch((err) => setError(err instanceof Error ? err.message : t("containerLogs.couldntRead")))
      .finally(() => setLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverId, containerName]);

  useEffect(load, [load]);

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal-panel" style={{ width: 720 }} onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2 className="modal-title">{t("containerLogs.title", { name: containerName })}</h2>
          <button className="modal-close" onClick={onClose} aria-label={t("common.close")}>
            <Icon name="x" size={16} />
          </button>
        </div>
        <div className="modal-body">
          {error && (
            <p className="form-note" style={{ color: "var(--danger)", marginBottom: 12 }}>
              {error}
            </p>
          )}
          <pre
            style={{
              height: 420,
              overflow: "auto",
              margin: 0,
              padding: 12,
              background: "var(--surface-bg)",
              border: "1px solid var(--border)",
              borderRadius: "var(--radius-sm)",
              fontFamily: "var(--font-mono)",
              fontSize: 12,
              lineHeight: 1.5,
              color: "var(--text-primary)",
              whiteSpace: "pre-wrap",
              wordBreak: "break-all",
            }}
          >
            {loading ? t("containerLogs.loading") : logs || t("containerLogs.noOutput", { lines: TAIL_LINES })}
          </pre>

          <div className="form-actions" style={{ marginTop: 12, gap: 8 }}>
            <Button variant="secondary" onClick={onClose}>
              {t("common.close")}
            </Button>
            <Button onClick={load} disabled={loading}>
              <Icon name="refresh-cw" size={14} />
              {t("common.refresh")}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
