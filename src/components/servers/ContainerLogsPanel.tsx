import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { useBackdropClose } from "@/hooks/useBackdropClose";
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
  const backdrop = useBackdropClose(onClose);

  return (
    <div className="modal-backdrop" {...backdrop}>
      <div className="modal-panel modal-panel-lg" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2 className="modal-title">{t("containerLogs.title", { name: containerName })}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <div className="modal-body">
          {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
          <pre className="container-logs-output">
            {loading ? t("containerLogs.loading") : logs || t("containerLogs.noOutput", { lines: TAIL_LINES })}
          </pre>

          <div className="form-actions form-actions-spaced">
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
