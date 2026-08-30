import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import type { Application, ApplicationStatus } from "@/types/application";
import "./ApplicationCard.css";

const STATUS_TONE: Record<ApplicationStatus, "neutral" | "success" | "danger" | "warning"> = {
  unknown: "neutral",
  starting: "warning",
  running: "success",
  stopping: "warning",
  stopped: "neutral",
  failed: "danger",
};

interface ApplicationCardProps {
  application: Application;
  serverName?: string;
  busy: boolean;
  onStart: () => void;
  onStop: () => void;
  onRestart: () => void;
  onKill: () => void;
  onDelete: () => void;
}

export function ApplicationCard({ application, serverName, busy, onStart, onStop, onRestart, onKill, onDelete }: ApplicationCardProps) {
  const { t } = useTranslation();
  const isLocal = !application.serverId;
  const canStart = application.status === "stopped" || application.status === "failed" || application.status === "unknown";
  const canStopOrRestart = application.status === "running" || application.status === "starting";

  return (
    <Card className="application-card">
      <div className="application-card-header">
        <div className="application-card-icon">
          <Icon name="box" size={16} />
        </div>
        <div className="application-card-title-col">
          <p className="application-card-name" title={application.name}>
            {application.name}
          </p>
          <p className="application-card-meta">
            {isLocal ? t("applicationCard.local") : (serverName ?? t("applicationCard.remote"))} · {application.blueprintId}
          </p>
        </div>
        <Badge tone={STATUS_TONE[application.status]}>{t(`applicationStatus.${application.status}`)}</Badge>
      </div>

      <div className="application-card-actions">
        {canStart && (
          <IconButton icon="play" size="sm" title={t("applicationCard.startAria", { name: application.name })} onClick={onStart} disabled={busy} />
        )}
        {canStopOrRestart && (
          <>
            <IconButton icon="square" size="sm" title={t("applicationCard.stopAria", { name: application.name })} onClick={onStop} disabled={busy} />
            <IconButton icon="refresh-cw" size="sm" title={t("applicationCard.restartAria", { name: application.name })} onClick={onRestart} disabled={busy} />
            <IconButton icon="power" size="sm" danger title={t("applicationCard.killAria", { name: application.name })} onClick={onKill} disabled={busy} />
          </>
        )}
        <IconButton icon="trash" size="sm" danger title={t("applicationCard.deleteAria", { name: application.name })} onClick={onDelete} disabled={busy} />
      </div>
    </Card>
  );
}
