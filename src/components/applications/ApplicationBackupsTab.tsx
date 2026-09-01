import { useCallback, useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { save } from "@tauri-apps/plugin-dialog";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { Switch } from "@/components/ui/Switch";
import { useBackdropClose } from "@/hooks/useBackdropClose";
import { downloadApplicationFile } from "@/services/applicationFilesService";
import {
  createApplicationBackup,
  deleteApplicationBackup,
  getApplicationBackupSchedule,
  listApplicationBackups,
  restoreApplicationBackup,
  setApplicationBackupSchedule,
} from "@/services/applicationBackupService";
import { toastSuccess } from "@/stores/toastStore";
import type { ApplicationBackup, ApplicationStatus, BackupSchedule } from "@/types/application";
import "@/components/servers/forms.css";
import "./ApplicationBackupsTab.css";

interface ApplicationBackupsTabProps {
  applicationId: string;
  applicationStatus: ApplicationStatus;
}

function backupPath(backup: ApplicationBackup): string {
  return `.vibessh-backups/${backup.fileName}`;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** A `.zip` of the working directory, manual ("Utwórz backup teraz") or on
 * an interval schedule - see the Rust `application_backup_service`'s own
 * doc comment for why a schedule only actually runs while VibeSSH is open,
 * surfaced here via `scheduleNote` rather than left implicit. */
export function ApplicationBackupsTab({ applicationId, applicationStatus }: ApplicationBackupsTabProps) {
  const { t } = useTranslation();
  const isStopped = applicationStatus === "stopped" || applicationStatus === "unknown" || applicationStatus === "failed";

  const [backups, setBackups] = useState<ApplicationBackup[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [downloadingId, setDownloadingId] = useState<string | null>(null);

  const [restoreTarget, setRestoreTarget] = useState<ApplicationBackup | null>(null);
  const [restoreBusy, setRestoreBusy] = useState(false);
  const [restoreError, setRestoreError] = useState<string | null>(null);
  const restoreBackdrop = useBackdropClose(() => !restoreBusy && setRestoreTarget(null));

  const [deleteTarget, setDeleteTarget] = useState<ApplicationBackup | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const deleteBackdrop = useBackdropClose(() => !deleteBusy && setDeleteTarget(null));

  const [schedule, setSchedule] = useState<BackupSchedule | null>(null);
  const [scheduleBusy, setScheduleBusy] = useState(false);
  const [scheduleError, setScheduleError] = useState<string | null>(null);

  const reload = useCallback(() => {
    setLoading(true);
    setError(null);
    listApplicationBackups(applicationId)
      .then(setBackups)
      .catch((err) => setError(err instanceof Error ? err.message : t("applicationBackups.loadError")))
      .finally(() => setLoading(false));
  }, [applicationId, t]);

  useEffect(reload, [reload]);
  useEffect(() => {
    getApplicationBackupSchedule(applicationId).then(setSchedule).catch(() => {});
  }, [applicationId]);

  async function handleCreate() {
    setCreating(true);
    setError(null);
    try {
      await createApplicationBackup(applicationId);
      toastSuccess(t("applicationBackups.createdToast"));
      reload();
    } catch (err) {
      setError(err instanceof Error ? err.message : t("applicationBackups.createError"));
    } finally {
      setCreating(false);
    }
  }

  async function handleDownload(backup: ApplicationBackup) {
    const localDest = await save({ defaultPath: backup.fileName, title: t("applicationBackups.downloadTitle") });
    if (!localDest) return;
    setDownloadingId(backup.id);
    try {
      await downloadApplicationFile(applicationId, backupPath(backup), localDest, crypto.randomUUID());
      toastSuccess(t("applicationBackups.downloadedToast"));
    } catch (err) {
      setError(err instanceof Error ? err.message : t("applicationBackups.downloadError"));
    } finally {
      setDownloadingId(null);
    }
  }

  async function handleConfirmRestore() {
    if (!restoreTarget) return;
    setRestoreBusy(true);
    setRestoreError(null);
    try {
      await restoreApplicationBackup(applicationId, restoreTarget.id);
      toastSuccess(t("applicationBackups.restoredToast"));
      setRestoreTarget(null);
    } catch (err) {
      setRestoreError(err instanceof Error ? err.message : t("applicationBackups.restoreError"));
    } finally {
      setRestoreBusy(false);
    }
  }

  async function handleConfirmDelete() {
    if (!deleteTarget) return;
    setDeleteBusy(true);
    setDeleteError(null);
    try {
      await deleteApplicationBackup(applicationId, deleteTarget.id);
      setDeleteTarget(null);
      reload();
    } catch (err) {
      setDeleteError(err instanceof Error ? err.message : t("applicationBackups.deleteError"));
    } finally {
      setDeleteBusy(false);
    }
  }

  async function handleSaveSchedule(e: FormEvent) {
    e.preventDefault();
    if (!schedule) return;
    setScheduleBusy(true);
    setScheduleError(null);
    try {
      setSchedule(await setApplicationBackupSchedule(applicationId, schedule));
      toastSuccess(t("applicationBackups.scheduleSavedToast"));
    } catch (err) {
      setScheduleError(err instanceof Error ? err.message : t("applicationBackups.scheduleSaveError"));
    } finally {
      setScheduleBusy(false);
    }
  }

  return (
    <div className="application-detail-overview">
      <Card title={t("applicationBackups.scheduleTitle")}>
        {schedule && (
          <form className="server-form" onSubmit={handleSaveSchedule}>
            {scheduleError && <p className="form-note form-note-danger form-note-spaced">{scheduleError}</p>}
            <Switch
              checked={schedule.enabled}
              onChange={(enabled) => setSchedule({ ...schedule, enabled })}
              label={t("applicationBackups.scheduleEnable")}
            />
            {schedule.enabled && (
              <div className="form-row">
                <label className="form-field">
                  <span className="form-label">{t("applicationBackups.scheduleInterval")}</span>
                  <input
                    className="form-input"
                    type="number"
                    min={1}
                    value={schedule.intervalHours}
                    onChange={(e) => setSchedule({ ...schedule, intervalHours: Number(e.target.value) })}
                  />
                </label>
                <label className="form-field">
                  <span className="form-label">{t("applicationBackups.scheduleRetention")}</span>
                  <input
                    className="form-input"
                    type="number"
                    min={1}
                    value={schedule.retentionCount}
                    onChange={(e) => setSchedule({ ...schedule, retentionCount: Number(e.target.value) })}
                  />
                </label>
              </div>
            )}
            {schedule.enabled && (
              <div className="form-row">
                <label className="form-field">
                  <span className="form-label">{t("applicationBackups.scheduleMaxAge")}</span>
                  <input
                    className="form-input"
                    type="number"
                    min={1}
                    value={schedule.retentionMaxAgeDays ?? ""}
                    onChange={(e) => setSchedule({ ...schedule, retentionMaxAgeDays: e.target.value ? Number(e.target.value) : undefined })}
                    placeholder={t("applicationBackups.scheduleMaxAgePlaceholder")}
                  />
                </label>
                <label className="form-field">
                  <span className="form-label">{t("applicationBackups.scheduleMaxSize")}</span>
                  <input
                    className="form-input"
                    type="number"
                    min={1}
                    value={schedule.retentionMaxTotalBytes ? Math.round(schedule.retentionMaxTotalBytes / (1024 * 1024)) : ""}
                    onChange={(e) =>
                      setSchedule({ ...schedule, retentionMaxTotalBytes: e.target.value ? Number(e.target.value) * 1024 * 1024 : undefined })
                    }
                    placeholder={t("applicationBackups.scheduleMaxSizePlaceholder")}
                  />
                </label>
              </div>
            )}
            {schedule.enabled && <p className="form-note">{t("applicationBackups.scheduleRetentionNote")}</p>}
            <p className="form-note">{t("applicationBackups.scheduleNote")}</p>
            <div className="form-actions">
              <Button type="submit" size="sm" disabled={scheduleBusy}>
                {scheduleBusy ? t("common.saving") : t("common.save")}
              </Button>
            </div>
          </form>
        )}
      </Card>

      <Card title={t("applicationBackups.title")}>
        <div className="application-backups-toolbar">
          <Button size="sm" onClick={handleCreate} disabled={creating}>
            <Icon name="archive" size={14} />
            {creating ? t("applicationBackups.creating") : t("applicationBackups.createNow")}
          </Button>
        </div>
        {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
        {loading ? (
          <SkeletonRows />
        ) : backups.length === 0 ? (
          <EmptyState icon="archive" title={t("applicationBackups.emptyTitle")} description={t("applicationBackups.emptyDescription")} />
        ) : (
          <ul className="server-list">
            {backups.map((backup) => (
              <li key={backup.id} className="server-list-item">
                <div className="server-list-icon">
                  <Icon name="archive" size={16} />
                </div>
                <span className="files-entry-name">{new Date(backup.createdAt).toLocaleString()}</span>
                <Badge tone={backup.kind === "manual" ? "neutral" : "success"}>
                  {backup.kind === "manual" ? t("applicationBackups.kindManual") : t("applicationBackups.kindScheduled")}
                </Badge>
                {backup.s3Key && (
                  <span title={t("applicationBackups.uploadedToDestination")}>
                    <Icon name="cloud" size={14} />
                  </span>
                )}
                <span className="files-entry-size">{formatSize(backup.sizeBytes)}</span>
                <IconButton
                  icon="download"
                  size="sm"
                  title={t("applicationBackups.downloadAria")}
                  disabled={downloadingId === backup.id}
                  onClick={() => handleDownload(backup)}
                />
                <IconButton
                  icon="refresh-cw"
                  size="sm"
                  title={isStopped ? t("applicationBackups.restoreAria") : t("applicationBackups.restoreDisabledAria")}
                  disabled={!isStopped}
                  onClick={() => setRestoreTarget(backup)}
                />
                <IconButton icon="trash" size="sm" title={t("applicationBackups.deleteAria")} onClick={() => setDeleteTarget(backup)} />
              </li>
            ))}
          </ul>
        )}
      </Card>

      {restoreTarget && (
        <div className="modal-backdrop" {...restoreBackdrop}>
          <div className="modal-panel modal-panel-sm" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2 className="modal-title">{t("applicationBackups.restoreTitle")}</h2>
              <IconButton icon="x" size="sm" onClick={() => setRestoreTarget(null)} title={t("common.close")} />
            </div>
            <div className="modal-body">
              <p className="dialog-body-text">{t("applicationBackups.restoreBody", { date: new Date(restoreTarget.createdAt).toLocaleString() })}</p>
              {restoreError && <p className="form-note form-note-danger form-note-spaced">{restoreError}</p>}
              <div className="form-actions">
                <Button variant="secondary" onClick={() => setRestoreTarget(null)} disabled={restoreBusy}>
                  {t("common.cancel")}
                </Button>
                <Button variant="danger" onClick={handleConfirmRestore} disabled={restoreBusy}>
                  {restoreBusy ? t("common.loading") : t("applicationBackups.restoreConfirm")}
                </Button>
              </div>
            </div>
          </div>
        </div>
      )}

      {deleteTarget && (
        <div className="modal-backdrop" {...deleteBackdrop}>
          <div className="modal-panel modal-panel-sm" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2 className="modal-title">{t("applicationBackups.deleteTitle")}</h2>
              <IconButton icon="x" size="sm" onClick={() => setDeleteTarget(null)} title={t("common.close")} />
            </div>
            <div className="modal-body">
              <p className="dialog-body-text">{t("applicationBackups.deleteBody", { date: new Date(deleteTarget.createdAt).toLocaleString() })}</p>
              {deleteError && <p className="form-note form-note-danger form-note-spaced">{deleteError}</p>}
              <div className="form-actions">
                <Button variant="secondary" onClick={() => setDeleteTarget(null)} disabled={deleteBusy}>
                  {t("common.cancel")}
                </Button>
                <Button variant="danger" onClick={handleConfirmDelete} disabled={deleteBusy}>
                  {t("common.remove")}
                </Button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
