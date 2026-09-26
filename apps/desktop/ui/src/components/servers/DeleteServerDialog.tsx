import { useCallback, useEffect, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { useModalDialog } from "@/hooks/useModalDialog";
import { deleteApplication, killApplication, listApplications, stopApplication } from "@/services/applicationService";
import { errorMessage } from "@/services/tauri";
import type { Application } from "@/types/application";
import "./AddServerModal.css";
import "./forms.css";
import "./DeleteServerDialog.css";

interface DeleteServerDialogProps {
  serverId: string;
  serverName: string;
  busy: boolean;
  error: string | null;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * Deleting a server, and what stands in its way.
 *
 * A server with applications on it is refused - their containers would go on
 * running on a Node nothing manages. So the applications are listed here, as
 * they are right now, with what can be done to each: stop it, kill it, delete
 * it. "Force delete" does the last to all of them and then deletes the
 * server, behind a second confirmation that says what goes and what stays.
 */
export function DeleteServerDialog({ serverId, serverName, busy, error, onConfirm, onCancel }: DeleteServerDialogProps) {
  const { t } = useTranslation();
  const backdrop = useModalDialog(onCancel, { labelledBy: "deleteserverdialog-dialog-title-1" });
  const [applications, setApplications] = useState<Application[] | null>(null);
  /** The application an action is running on, or "all" while forcing. */
  const [working, setWorking] = useState<string | null>(null);
  const [confirmingDelete, setConfirmingDelete] = useState<string | null>(null);
  const [confirmingForce, setConfirmingForce] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const all = await listApplications();
      setApplications(all.filter((application) => application.serverId === serverId));
    } catch (err) {
      setActionError(errorMessage(err, t));
    }
  }, [serverId, t]);

  useEffect(() => {
    void load();
  }, [load]);

  async function act(application: Application, action: "stop" | "kill" | "delete") {
    setWorking(application.id);
    setActionError(null);
    try {
      if (action === "stop") await stopApplication(application.id, true);
      if (action === "kill") await killApplication(application.id);
      if (action === "delete") await deleteApplication(application.id, false);
    } catch (err) {
      setActionError(t("deleteDialog.actionFailed", { name: application.name, reason: errorMessage(err, t) }));
    } finally {
      setConfirmingDelete(null);
      setWorking(null);
      await load();
    }
  }

  /** Every application first, then the server - stopping at the first
   *  application that will not go, since the server would be refused anyway. */
  async function forceDelete() {
    if (!applications) return;
    setWorking("all");
    setActionError(null);
    for (const application of applications) {
      try {
        await deleteApplication(application.id, false);
      } catch (err) {
        setActionError(t("deleteDialog.actionFailed", { name: application.name, reason: errorMessage(err, t) }));
        setWorking(null);
        setConfirmingForce(false);
        await load();
        return;
      }
    }
    setWorking(null);
    setConfirmingForce(false);
    onConfirm();
  }

  const hasApplications = (applications?.length ?? 0) > 0;
  const locked = busy || working !== null;

  return (
    <div className="modal-backdrop" {...backdrop.backdropProps}>
      <div className="modal-panel" {...backdrop.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="deleteserverdialog-dialog-title-1">{t("deleteDialog.title")}</h2>
          <IconButton icon="x" size="sm" onClick={onCancel} title={t("common.close")} disabled={locked} />
        </div>
        <div className="modal-body">
          <p className="dialog-body-text">
            <Trans i18nKey="deleteDialog.body" values={{ name: serverName }} components={{ 1: <strong /> }} />
          </p>

          {hasApplications && (
            <div className="delete-server-apps">
              <p className="form-note">{t("deleteDialog.appsIntro", { count: applications?.length ?? 0 })}</p>
              <ul className="delete-server-app-list">
                {applications?.map((application) => {
                  const running = application.status === "running" || application.status === "starting";
                  return (
                    <li key={application.id} className="delete-server-app">
                      <span className="delete-server-app-name">{application.name}</span>
                      <Badge tone={running ? "success" : "neutral"}>{t(`applicationStatus.${application.status}`, application.status)}</Badge>
                      <span className="delete-server-app-actions">
                        {confirmingDelete === application.id ? (
                          <>
                            <Button size="sm" variant="secondary" onClick={() => setConfirmingDelete(null)} disabled={locked}>
                              {t("common.cancel")}
                            </Button>
                            <Button size="sm" variant="danger" onClick={() => void act(application, "delete")} disabled={locked}>
                              {working === application.id ? t("common.loading") : t("deleteDialog.confirmDeleteApp")}
                            </Button>
                          </>
                        ) : (
                          <>
                            {running && (
                              <Button size="sm" variant="secondary" onClick={() => void act(application, "stop")} disabled={locked}>
                                {t("deleteDialog.stop")}
                              </Button>
                            )}
                            {running && (
                              <Button size="sm" variant="secondary" onClick={() => void act(application, "kill")} disabled={locked}>
                                {t("deleteDialog.kill")}
                              </Button>
                            )}
                            <Button size="sm" variant="secondary" onClick={() => setConfirmingDelete(application.id)} disabled={locked}>
                              {t("deleteDialog.deleteApp")}
                            </Button>
                          </>
                        )}
                      </span>
                    </li>
                  );
                })}
              </ul>
            </div>
          )}

          {actionError && <p className="form-note form-note-danger form-note-spaced">{actionError}</p>}
          {error && !hasApplications && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
          {confirmingForce && <p className="form-note form-note-danger form-note-spaced">{t("deleteDialog.forceWarning", { count: applications?.length ?? 0 })}</p>}

          <div className="form-actions">
            <Button variant="secondary" onClick={confirmingForce ? () => setConfirmingForce(false) : onCancel} disabled={locked}>
              {t("common.cancel")}
            </Button>
            {hasApplications ? (
              <Button variant="danger" onClick={confirmingForce ? () => void forceDelete() : () => setConfirmingForce(true)} disabled={locked}>
                {working === "all" ? t("deleteDialog.forcing") : confirmingForce ? t("deleteDialog.forceConfirm") : t("deleteDialog.force")}
              </Button>
            ) : (
              <Button variant="danger" onClick={onConfirm} disabled={locked || applications === null}>
                {t("common.remove")}
              </Button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
