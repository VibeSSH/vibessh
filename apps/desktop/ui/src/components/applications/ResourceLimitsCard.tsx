import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { setApplicationResourceLimits } from "@/services/applicationService";
import { installCron, setApplicationDiskLimit } from "@/services/scheduleService";
import { useContainerApply } from "@/hooks/useContainerApply";
import type { ApplicationDetail, ResourceLimitsConfig } from "@/types/application";
import "@/components/servers/forms.css";
import { CommandError, errorMessage } from "@/services/tauri";
import { toastSuccess } from "@/stores/toastStore";

interface ResourceLimitsCardProps {
  applicationId: string;
  application: ApplicationDetail;
  onApplied: (updated: ApplicationDetail) => void;
  /** A Docker application on a Node over SSH - the only kind the Node can measure and stop. */
  diskLimitAvailable?: boolean;
}

/** Megabytes as the gigabytes the form shows, without a trailing `.0`. */
function toGb(mb: number | undefined): string {
  return mb ? String(Math.round((mb / 1024) * 10) / 10) : "";
}

/** Rendered for Docker/systemd/Remote Process applications (see `ApplicationDetail`'s own gating) - Local Process is the only one with no OS-level mechanism VibeSSH can enforce even a CPU limit through, so it's the one left out. Remote Process only gets the CPU field: it has no cgroup of its own to cap memory through the way Docker/systemd do (see `runtime::remote_process`'s `cpulimit`-based CPU cap for why CPU alone is still possible there). The disk limit is different in kind - the Node measures the directory and stops the server when it is over, rather than the runtime capping it - and is offered only where the Node can do that. */
export function ResourceLimitsCard({ applicationId, application, onApplied, diskLimitAvailable = false }: ResourceLimitsCardProps) {
  const { t } = useTranslation();
  const applyToContainer = useContainerApply();
  const config = (application.runtimeConfig ?? {}) as ResourceLimitsConfig;
  const supportsMemory = application.runtimeType !== "remoteProcess";

  const [editing, setEditing] = useState(false);
  const [memoryLimitMb, setMemoryLimitMb] = useState(config.memoryLimitMb ? String(config.memoryLimitMb) : "");
  const [cpuLimitCores, setCpuLimitCores] = useState(config.cpuLimitCores ? String(config.cpuLimitCores) : "");
  const [diskLimitGb, setDiskLimitGb] = useState(toGb(config.diskLimitMb));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** The Node to install cron on, when saving a disk limit found it has none. */
  const [cronMissingOn, setCronMissingOn] = useState<string | null>(null);
  const [installingCron, setInstallingCron] = useState(false);

  function startEditing() {
    setMemoryLimitMb(config.memoryLimitMb ? String(config.memoryLimitMb) : "");
    setCpuLimitCores(config.cpuLimitCores ? String(config.cpuLimitCores) : "");
    setDiskLimitGb(toGb(config.diskLimitMb));
    setError(null);
    setEditing(true);
  }

  async function handleSubmit(e?: React.FormEvent) {
    e?.preventDefault();
    setCronMissingOn(null);
    const memory = supportsMemory && memoryLimitMb.trim() ? Number(memoryLimitMb) : undefined;
    const cpu = cpuLimitCores.trim() ? Number(cpuLimitCores) : undefined;
    const diskGb = diskLimitAvailable && diskLimitGb.trim() ? Number(diskLimitGb.replace(",", ".")) : undefined;
    if (
      (memory !== undefined && (!Number.isFinite(memory) || memory <= 0)) ||
      (cpu !== undefined && (!Number.isFinite(cpu) || cpu <= 0)) ||
      (diskGb !== undefined && (!Number.isFinite(diskGb) || diskGb <= 0))
    ) {
      setError(t("resourceLimits.invalidForm"));
      return;
    }
    const diskMb = diskGb === undefined ? null : Math.round(diskGb * 1024);

    setBusy(true);
    setError(null);
    try {
      let updated = application;
      // Memory and CPU are baked into the container at `docker create`, so a
      // change there needs a recreate - which runs in the background via
      // `useContainerApply` rather than freezing this form. The disk limit
      // needs none: the Node enforces it, so saving it alone recreates nothing.
      const cpuOrMemoryChanged = memory !== config.memoryLimitMb || cpu !== config.cpuLimitCores;
      if (cpuOrMemoryChanged) {
        updated = await setApplicationResourceLimits(applicationId, { memoryLimitMb: memory, cpuLimitCores: cpu });
      }
      if (diskLimitAvailable && diskMb !== (config.diskLimitMb ?? null)) {
        updated = await setApplicationDiskLimit(applicationId, diskMb);
      }
      setEditing(false);
      onApplied(updated);
      if (cpuOrMemoryChanged) void applyToContainer(updated);
    } catch (err) {
      setError(errorMessage(err, t));
      if (err instanceof CommandError && err.code === "cron_missing" && typeof err.params.serverId === "string") {
        setCronMissingOn(err.params.serverId);
      }
    } finally {
      setBusy(false);
    }
  }

  /** Installs cron, then saves again - the save is what the person asked for. */
  async function installAndRetry() {
    if (!cronMissingOn) return;
    setInstallingCron(true);
    try {
      await installCron(cronMissingOn);
      toastSuccess(t("schedules.cronInstalledToast"));
      await handleSubmit();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setInstallingCron(false);
    }
  }

  return (
    <Card
      title={t("resourceLimits.title")}
      actions={
        !editing ? (
          <Button variant="secondary" size="sm" onClick={startEditing}>
            <Icon name="edit" size={14} />
            {t("resourceLimits.edit")}
          </Button>
        ) : undefined
      }
    >
      {application.runtimeType === "docker" && (
        <p className="form-note">
          {application.status === "running" ? t("applicationConfig.recreateAutoNote") : t("applicationConfig.recreateStoppedNote")}
        </p>
      )}
      {application.runtimeType === "remoteProcess" && <p className="form-note">{t("resourceLimits.remoteProcessNote")}</p>}

      {editing ? (
        <form className="server-form" onSubmit={handleSubmit}>
          {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
          <div className="form-row">
            {supportsMemory && (
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
            )}
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
            {diskLimitAvailable && (
              <label className="form-field">
                <span className="form-label">{t("resourceLimits.disk")}</span>
                <input
                  className="form-input"
                  type="number"
                  min={0.1}
                  step={0.5}
                  value={diskLimitGb}
                  onChange={(e) => setDiskLimitGb(e.target.value)}
                  placeholder={t("resourceLimits.diskPlaceholder")}
                />
              </label>
            )}
          </div>
          {diskLimitAvailable && <p className="form-note">{t("resourceLimits.diskNote")}</p>}
          {cronMissingOn && (
            <Button type="button" variant="secondary" size="sm" onClick={() => void installAndRetry()} disabled={installingCron || busy}>
              <Icon name="download" size={14} />
              {installingCron ? t("schedules.installingCron") : t("schedules.installCron")}
            </Button>
          )}
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
            {supportsMemory && (
              <>
                <span className="wizard-review-label">{t("resourceLimits.memory")}</span>
                <span className="wizard-review-value">{config.memoryLimitMb ? `${config.memoryLimitMb} MB` : t("resourceLimits.unlimited")}</span>
              </>
            )}
            <span className="wizard-review-label">{t("resourceLimits.cpu")}</span>
            <span className="wizard-review-value">
              {config.cpuLimitCores ? t("resourceLimits.cores", { count: config.cpuLimitCores }) : t("resourceLimits.unlimited")}
            </span>
            {diskLimitAvailable && (
              <>
                <span className="wizard-review-label">{t("resourceLimits.disk")}</span>
                <span className="wizard-review-value">{config.diskLimitMb ? `${toGb(config.diskLimitMb)} GB` : t("resourceLimits.unlimited")}</span>
              </>
            )}
          </div>
        </>
      )}
    </Card>
  );
}
