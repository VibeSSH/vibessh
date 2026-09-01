import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { useBackdropClose } from "@/hooks/useBackdropClose";
import { getApplicationHealth, setApplicationHealthCheck } from "@/services/applicationService";
import type { ApplicationDetail, HealthCheckType, HealthStatus } from "@/types/application";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";
import { errorMessage } from "@/services/tauri";

const HEALTH_TONE: Record<HealthStatus["status"], "neutral" | "success" | "danger"> = {
  healthy: "success",
  unhealthy: "danger",
  unknown: "neutral",
};

interface HealthCheckCardProps {
  applicationId: string;
  application: ApplicationDetail;
  onConfigChanged: () => void;
}

/** A probe run on demand, not on the same 5s poll `ApplicationDetail` uses for status/resource usage - a `Tcp`/`Http`/`MinecraftStatus` check is a real network round trip (and, for a Remote application, a real SSH exec), not a free read, so it only runs when this card mounts, when its config changes, or when the user asks again. */
export function HealthCheckCard({ applicationId, application, onConfigChanged }: HealthCheckCardProps) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<HealthStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [checkError, setCheckError] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);

  const runCheck = useCallback(() => {
    setChecking(true);
    setCheckError(null);
    getApplicationHealth(applicationId)
      .then(setStatus)
      .catch((err) => setCheckError(errorMessage(err, t)))
      .finally(() => setChecking(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [applicationId, application.healthCheckType, application.healthCheckPortId, application.healthCheckHttpPath, t]);

  useEffect(runCheck, [runCheck]);

  const port = application.ports.find((p) => p.id === application.healthCheckPortId);

  return (
    <Card title={t("healthCheck.title")}>
      <div className="application-detail-header-row">
        <Badge tone={status ? HEALTH_TONE[status.status] : "neutral"}>
          {status ? t(`healthCheck.status.${status.status}`) : t("healthCheck.status.unknown")}
        </Badge>
        <div className="application-detail-actions">
          <Button variant="secondary" size="sm" onClick={runCheck} disabled={checking}>
            <Icon name="refresh-cw" size={14} />
            {t("healthCheck.checkNow")}
          </Button>
          <Button variant="secondary" size="sm" onClick={() => setEditing(true)}>
            <Icon name="edit" size={14} />
            {t("healthCheck.configure")}
          </Button>
        </div>
      </div>

      {status?.status === "unhealthy" && <p className="form-note form-note-danger form-note-spaced">{status.reason}</p>}
      {checkError && <p className="form-note form-note-danger form-note-spaced">{checkError}</p>}

      <div className="wizard-review-grid">
        <span className="wizard-review-label">{t("healthCheck.type")}</span>
        <span className="wizard-review-value">{t(`healthCheck.typeOption.${application.healthCheckType}`)}</span>
        {application.healthCheckType !== "process" && (
          <>
            <span className="wizard-review-label">{t("healthCheck.port")}</span>
            <span className="wizard-review-value">{port ? `${port.name} (${port.internalPort})` : t("healthCheck.portMissing")}</span>
          </>
        )}
        {application.healthCheckType === "http" && (
          <>
            <span className="wizard-review-label">{t("healthCheck.path")}</span>
            <span className="wizard-review-value">{application.healthCheckHttpPath ?? "—"}</span>
          </>
        )}
      </div>

      {editing && (
        <HealthCheckFormModal
          applicationId={applicationId}
          application={application}
          onClose={() => setEditing(false)}
          onSaved={() => {
            setEditing(false);
            onConfigChanged();
          }}
        />
      )}
    </Card>
  );
}

interface HealthCheckFormModalProps {
  applicationId: string;
  application: ApplicationDetail;
  onClose: () => void;
  onSaved: () => void;
}

function HealthCheckFormModal({ applicationId, application, onClose, onSaved }: HealthCheckFormModalProps) {
  const { t } = useTranslation();
  const backdrop = useBackdropClose(onClose);

  const [type, setType] = useState<HealthCheckType>(application.healthCheckType);
  const [portId, setPortId] = useState(application.healthCheckPortId ?? "");
  const [httpPath, setHttpPath] = useState(application.healthCheckHttpPath ?? "/");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Every check type here dials a raw TCP connection under the hood (a
  // plain connect, an HTTP request, or a Minecraft ping) - a UDP-declared
  // port isn't a valid target for any of them, so only TCP ports are
  // offered.
  const tcpPorts = application.ports.filter((p) => p.protocol === "tcp");
  const needsPort = type !== "process";

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (needsPort && !portId) {
      setError(t("healthCheck.formPortRequired"));
      return;
    }
    if (type === "http" && !httpPath.trim().startsWith("/")) {
      setError(t("healthCheck.formPathRequired"));
      return;
    }

    setBusy(true);
    setError(null);
    try {
      await setApplicationHealthCheck(applicationId, {
        healthCheckType: type,
        portId: needsPort ? portId : undefined,
        httpPath: type === "http" ? httpPath.trim() : undefined,
      });
      onSaved();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" {...backdrop}>
      <div className="modal-panel" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2 className="modal-title">{t("healthCheck.configure")}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <form className="server-form" onSubmit={handleSubmit}>
          <div className="modal-body">
            {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}

            <label className="form-field">
              <span className="form-label">{t("healthCheck.type")}</span>
              <select className="form-input" value={type} onChange={(e) => setType(e.target.value as HealthCheckType)}>
                <option value="process">{t("healthCheck.typeOption.process")}</option>
                <option value="tcp">{t("healthCheck.typeOption.tcp")}</option>
                <option value="http">{t("healthCheck.typeOption.http")}</option>
                <option value="minecraftStatus">{t("healthCheck.typeOption.minecraftStatus")}</option>
              </select>
            </label>

            {needsPort &&
              (tcpPorts.length === 0 ? (
                <p className="form-note">{t("healthCheck.noPorts")}</p>
              ) : (
                <label className="form-field">
                  <span className="form-label">{t("healthCheck.port")}</span>
                  <select className="form-input" value={portId} onChange={(e) => setPortId(e.target.value)}>
                    <option value="" disabled>
                      {t("healthCheck.choosePort")}
                    </option>
                    {tcpPorts.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name} ({p.internalPort})
                      </option>
                    ))}
                  </select>
                </label>
              ))}

            {type === "http" && (
              <label className="form-field">
                <span className="form-label">{t("healthCheck.path")}</span>
                <input className="form-input" value={httpPath} onChange={(e) => setHttpPath(e.target.value)} placeholder="/health" />
              </label>
            )}

            <div className="form-actions">
              <Button type="button" variant="secondary" onClick={onClose} disabled={busy}>
                {t("common.cancel")}
              </Button>
              <Button type="submit" disabled={busy || (needsPort && tcpPorts.length === 0)}>
                {busy ? t("common.saving") : t("common.save")}
              </Button>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}
