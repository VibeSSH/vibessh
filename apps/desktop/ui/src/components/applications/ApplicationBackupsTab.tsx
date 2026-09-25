import { useEffect, useState, type FormEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { queryKeys } from "@/services/queryKeys";
import { useTranslation } from "react-i18next";
import { save } from "@tauri-apps/plugin-dialog";
import { Badge } from "@/components/ui/Badge";
import { GuideLink } from "@/guide/GuideLink";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { Switch } from "@/components/ui/Switch";
import { useModalDialog } from "@/hooks/useModalDialog";
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
import { errorMessage } from "@/services/tauri";

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
  const { t, i18n } = useTranslation();
  const isStopped = applicationStatus === "stopped" || applicationStatus === "unknown" || applicationStatus === "failed";

  const queryClient = useQueryClient();
  // See DatabasesTab: the list's own failure and an action's failure are
  // different sentences and must not overwrite each other.
  const [actionError, setActionError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [downloadingId, setDownloadingId] = useState<string | null>(null);

  const [restoreTarget, setRestoreTarget] = useState<ApplicationBackup | null>(null);
  const [restoreBusy, setRestoreBusy] = useState(false);
  const [restoreError, setRestoreError] = useState<string | null>(null);
  const restoreBackdrop = useModalDialog(() => !restoreBusy && setRestoreTarget(null), { labelledBy: "applicationbackupstab-dialog-title-1" });

  const [deleteTarget, setDeleteTarget] = useState<ApplicationBackup | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const deleteBackdrop = useModalDialog(() => !deleteBusy && setDeleteTarget(null), { labelledBy: "applicationbackupstab-dialog-title-2" });

  const [schedule, setSchedule] = useState<BackupSchedule | null>(null);
  const [scheduleBusy, setScheduleBusy] = useState(false);
  const [scheduleError, setScheduleError] = useState<string | null>(null);
  // Shown as a summary until somebody asks to change it; the saved copy is
  // what Cancel goes back to.
  const [editingSchedule, setEditingSchedule] = useState(false);
  const [savedSchedule, setSavedSchedule] = useState<BackupSchedule | null>(null);

  const {
    data: backups = [],
    isPending: loading,
    error: loadError,
  } = useQuery({
    queryKey: queryKeys.applicationBackups(applicationId),
    queryFn: () => listApplicationBackups(applicationId),
  });

  const error = actionError ?? (loadError ? errorMessage(loadError, t) : null);

  const reload = () => {
    void queryClient.invalidateQueries({ queryKey: queryKeys.applicationBackups(applicationId) });
  };
  useEffect(() => {
    getApplicationBackupSchedule(applicationId)
      .then((loaded) => {
        setSchedule(loaded);
        setSavedSchedule(loaded);
      })
      .catch(() => {});
  }, [applicationId]);

  async function handleCreate() {
    setCreating(true);
    setActionError(null);
    try {
      await createApplicationBackup(applicationId);
      toastSuccess(t("applicationBackups.createdToast"));
      reload();
    } catch (err) {
      setActionError(errorMessage(err, t));
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
      setActionError(errorMessage(err, t));
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
      setRestoreError(errorMessage(err, t));
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
      setDeleteError(errorMessage(err, t));
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
      const saved = await setApplicationBackupSchedule(applicationId, schedule);
      setSchedule(saved);
      setSavedSchedule(saved);
      setEditingSchedule(false);
      toastSuccess(t("applicationBackups.scheduleSavedToast"));
    } catch (err) {
      setScheduleError(errorMessage(err, t));
    } finally {
      setScheduleBusy(false);
    }
  }

  const totalBytes = backups.reduce((sum, backup) => sum + backup.sizeBytes, 0);
  const newest = backups.reduce<ApplicationBackup | null>((latest, backup) => (!latest || backup.createdAt > latest.createdAt ? backup : latest), null);

  return (
    <div className="application-detail-overview">
      {/* The copies are what this tab is for, so they take the width; the
          schedule that makes them sits beside them as a summary, opened for
          editing on request. It used to be a form spread out above the list
          whether or not anybody meant to change it. */}
      <div className="application-detail-overview-grid">
        <div className="application-detail-overview">
          <Card
            title={t("applicationBackups.title")}
            subtitle={
              backups.length > 0
                ? t("applicationBackups.listSummary", {
                    count: backups.length,
                    size: formatSize(totalBytes),
                    when: newest ? new Date(newest.createdAt).toLocaleString(i18n.language, { dateStyle: "medium", timeStyle: "short" }) : "",
                  })
                : undefined
            }
            actions={
              <>
                <GuideLink topic="backups" />
                <Button size="sm" onClick={handleCreate} disabled={creating}>
                  <Icon name="archive" size={14} />
                  {creating ? t("applicationBackups.creating") : t("applicationBackups.createNow")}
                </Button>
              </>
            }
          >
            {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
            {!isStopped && backups.length > 0 && <p className="backups-restore-note">{t("applicationBackups.restoreNeedsStop")}</p>}
            {loading ? (
              <SkeletonRows />
            ) : backups.length === 0 ? (
              <EmptyState icon="archive" title={t("applicationBackups.emptyTitle")} description={t("applicationBackups.emptyDescription")} />
            ) : (
              <>
                <div className="backup-row backup-row-head" role="presentation">
                  <span />
                  <span className="backup-head-cell">{t("applicationBackups.columnDate")}</span>
                  <span className="backup-head-cell">{t("applicationBackups.columnKind")}</span>
                  <span className="backup-head-cell backup-cell-end">{t("applicationBackups.columnSize")}</span>
                  <span className="backup-head-cell">{t("applicationBackups.columnWhere")}</span>
                  <span />
                </div>
                <ul className="backup-rows">
                  {backups.map((backup) => (
                    <li key={backup.id} className="backup-row">
                      <span className="backup-row-icon">
                        <Icon name="archive" size={15} />
                      </span>
                      <span className="backup-row-date">
                        {new Date(backup.createdAt).toLocaleString(i18n.language, { dateStyle: "medium", timeStyle: "short" })}
                      </span>
                      <span>
                        <Badge tone={backup.kind === "manual" ? "neutral" : "success"}>
                          {backup.kind === "manual" ? t("applicationBackups.kindManual") : t("applicationBackups.kindScheduled")}
                        </Badge>
                      </span>
                      <span className="backup-cell backup-cell-end">{formatSize(backup.sizeBytes)}</span>
                      <span className="backup-cell backup-where">
                        {backup.s3Key ? (
                          <>
                            <Icon name="cloud" size={13} />
                            {t("applicationBackups.whereLocalAndCloud")}
                          </>
                        ) : (
                          t("applicationBackups.whereLocal")
                        )}
                      </span>
                      <span className="backup-row-actions">
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
                        <IconButton icon="trash" size="sm" danger title={t("applicationBackups.deleteAria")} onClick={() => setDeleteTarget(backup)} />
                      </span>
                    </li>
                  ))}
                </ul>
              </>
            )}
          </Card>
        </div>

        <aside className="application-detail-aside">
          <Card
            title={t("applicationBackups.scheduleTitle")}
            className="backups-schedule-card"
            actions={
              schedule && !editingSchedule ? (
                <Button variant="secondary" size="sm" onClick={() => setEditingSchedule(true)}>
                  <Icon name="edit" size={14} />
                  {t("applicationBackups.scheduleEdit")}
                </Button>
              ) : undefined
            }
          >
            {schedule && !editingSchedule && (
              <div className="backups-schedule-summary">
                <Badge tone={schedule.enabled ? "success" : "neutral"}>
                  {schedule.enabled ? t("applicationBackups.scheduleOn") : t("applicationBackups.scheduleOff")}
                </Badge>
                {schedule.enabled ? (
                  <dl className="backups-schedule-facts">
                    <dt>{t("applicationBackups.scheduleInterval")}</dt>
                    <dd>{t("applicationBackups.everyHours", { count: schedule.intervalHours })}</dd>
                    <dt>{t("applicationBackups.scheduleRetention")}</dt>
                    <dd>{schedule.retentionCount}</dd>
                    <dt>{t("applicationBackups.scheduleMaxAge")}</dt>
                    <dd>{schedule.retentionMaxAgeDays ? t("applicationBackups.days", { count: schedule.retentionMaxAgeDays }) : "—"}</dd>
                    <dt>{t("applicationBackups.scheduleMaxSize")}</dt>
                    <dd>{schedule.retentionMaxTotalBytes ? formatSize(schedule.retentionMaxTotalBytes) : "—"}</dd>
                  </dl>
                ) : (
                  <p className="form-note">{t("applicationBackups.scheduleOffNote")}</p>
                )}
              </div>
            )}
            {schedule && editingSchedule && (
              <form className="server-form" onSubmit={handleSaveSchedule}>
                {scheduleError && <p className="form-note form-note-danger form-note-spaced">{scheduleError}</p>}
                <Switch
                  checked={schedule.enabled}
                  onChange={(enabled) => setSchedule({ ...schedule, enabled })}
                  label={t("applicationBackups.scheduleEnable")}
                />
                {schedule.enabled && (
                  <>
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
                    <p className="form-note">{t("applicationBackups.scheduleRetentionNote")}</p>
                  </>
                )}
                <p className="form-note">{t("applicationBackups.scheduleNote")}</p>
                <div className="form-actions">
                  <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    disabled={scheduleBusy}
                    onClick={() => {
                      setSchedule(savedSchedule);
                      setScheduleError(null);
                      setEditingSchedule(false);
                    }}
                  >
                    {t("common.cancel")}
                  </Button>
                  <Button type="submit" size="sm" disabled={scheduleBusy}>
                    {scheduleBusy ? t("common.saving") : t("common.save")}
                  </Button>
                </div>
              </form>
            )}
          </Card>
        </aside>
      </div>

      {restoreTarget && (
        <div className="modal-backdrop" {...restoreBackdrop.backdropProps}>
          <div className="modal-panel modal-panel-sm" {...restoreBackdrop.panelProps}>
            <div className="modal-header">
              <h2 className="modal-title" id="applicationbackupstab-dialog-title-1">{t("applicationBackups.restoreTitle")}</h2>
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
        <div className="modal-backdrop" {...deleteBackdrop.backdropProps}>
          <div className="modal-panel modal-panel-sm" {...deleteBackdrop.panelProps}>
            <div className="modal-header">
              <h2 className="modal-title" id="applicationbackupstab-dialog-title-2">{t("applicationBackups.deleteTitle")}</h2>
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
