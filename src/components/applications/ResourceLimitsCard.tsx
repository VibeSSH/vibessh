import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { setApplicationResourceLimits } from "@/services/applicationService";
import type { ApplicationDetail, ResourceLimitsConfig } from "@/types/application";
import "@/components/servers/forms.css";

interface ResourceLimitsCardProps {
  applicationId: string;
  application: ApplicationDetail;
  onSaved: () => void;
}

/** Only rendered for a Docker/systemd application (see `ApplicationDetail`'s own gating) - there's no OS-level mechanism VibeSSH can enforce a memory/CPU limit through for a bare Local/Remote process, so the option isn't offered there rather than silently doing nothing. */
export function ResourceLimitsCard({ applicationId, application, onSaved }: ResourceLimitsCardProps) {
  const { t } = useTranslation();
  const config = (application.runtimeConfig ?? {}) as ResourceLimitsConfig;

  const [editing, setEditing] = useState(false);
  const [memoryLimitMb, setMemoryLimitMb] = useState(config.memoryLimitMb ? String(config.memoryLimitMb) : "");
  const [cpuLimitCores, setCpuLimitCores] = useState(config.cpuLimitCores ? String(config.cpuLimitCores) : "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  function startEditing() {
    setMemoryLimitMb(config.memoryLimitMb ? String(config.memoryLimitMb) : "");
    setCpuLimitCores(config.cpuLimitCores ? String(config.cpuLimitCores) : "");
    setError(null);
    setEditing(true);
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const memory = memoryLimitMb.trim() ? Number(memoryLimitMb) : undefined;
    const cpu = cpuLimitCores.trim() ? Number(cpuLimitCores) : undefined;
    if ((memory !== undefined && (!Number.isFinite(memory) || memory <= 0)) || (cpu !== undefined && (!Number.isFinite(cpu) || cpu <= 0))) {
      setError(t("resourceLimits.invalidForm"));
      return;
    }

    setBusy(true);
    setError(null);
    try {
      await setApplicationResourceLimits(applicationId, { memoryLimitMb: memory, cpuLimitCores: cpu });
      setEditing(false);
      onSaved();
    } catch (err) {
      setError(err instanceof Error ? err.message : t("resourceLimits.saveError"));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card title={t("resourceLimits.title")}>
      {application.runtimeType === "docker" && <p className="form-note">{t("resourceLimits.dockerNote")}</p>}

      {editing ? (
        <form className="server-form" onSubmit={handleSubmit}>
          {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
          <div className="form-row">
            <label className="form-field">
              <span className="form-label">{t("resourceLimits.memory")}</span>
              <input
                className="form-input"
                type="number"
                min={1}
                value={memoryLimitMb}
                onChange={(e) => setMemoryLimitMb(e.target.value)}
                placeholder={t("resourceLimits.memoryPlaceholder")}
              />
            </label>
            <label className="form-field">
              <span className="form-label">{t("resourceLimits.cpu")}</span>
              <input
                className="form-input"
                type="number"
                min={0.1}
                step={0.1}
                value={cpuLimitCores}
                onChange={(e) => setCpuLimitCores(e.target.value)}
                placeholder={t("resourceLimits.cpuPlaceholder")}
              />
            </label>
          </div>
          <div className="form-actions">
            <Button type="button" variant="secondary" onClick={() => setEditing(false)} disabled={busy}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={busy}>
              {busy ? t("common.saving") : t("common.save")}
            </Button>
          </div>
        </form>
      ) : (
        <>
          <div className="wizard-review-grid">
            <span className="wizard-review-label">{t("resourceLimits.memory")}</span>
            <span className="wizard-review-value">{config.memoryLimitMb ? `${config.memoryLimitMb} MB` : t("resourceLimits.unlimited")}</span>
            <span className="wizard-review-label">{t("resourceLimits.cpu")}</span>
            <span className="wizard-review-value">
              {config.cpuLimitCores ? t("resourceLimits.cores", { count: config.cpuLimitCores }) : t("resourceLimits.unlimited")}
            </span>
          </div>
          <div className="form-actions">
            <Button variant="secondary" size="sm" onClick={startEditing}>
              <Icon name="edit" size={14} />
              {t("resourceLimits.edit")}
            </Button>
          </div>
        </>
      )}
    </Card>
  );
}
