import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { ApplicationCard } from "@/components/applications/ApplicationCard";
import { CreateApplicationWizard } from "@/components/applications/CreateApplicationWizard";
import { DeleteApplicationDialog } from "@/components/applications/DeleteApplicationDialog";
import {
  deleteApplication,
  killApplication,
  listApplications,
  restartApplication,
  startApplication,
  stopApplication,
} from "@/services/applicationService";
import { listServers, serverSummaryToManagedServer } from "@/services/serverService";
import { useApplicationsStore } from "@/stores/applicationsStore";
import { useServersStore } from "@/stores/serversStore";
import { toastSuccess } from "@/stores/toastStore";
import type { Application } from "@/types/application";
import "./pages.css";
import "./Applications.css";

export function Applications() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const applications = useApplicationsStore((s) => s.applications);
  const setApplications = useApplicationsStore((s) => s.setApplications);
  const removeApplication = useApplicationsStore((s) => s.removeApplication);
  const servers = useServersStore((s) => s.servers);
  const setServers = useServersStore((s) => s.setServers);

  const [wizardOpen, setWizardOpen] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [deletingApplication, setDeletingApplication] = useState<Application | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  function reload() {
    listApplications()
      .then(setApplications)
      .catch(() => {
        // No applications yet, or this loaded outside a Tauri webview during development.
      });
  }

  useEffect(() => {
    reload();
    if (servers.length === 0) {
      listServers()
        .then((loaded) => setServers(loaded.map(serverSummaryToManagedServer)))
        .catch(() => {});
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function runAction(id: string, action: () => Promise<unknown>) {
    setActionError(null);
    setBusyId(id);
    try {
      await action();
      reload();
    } catch (err) {
      setActionError(err instanceof Error ? err.message : t("applications.actionError"));
    } finally {
      setBusyId(null);
    }
  }

  async function handleConfirmDelete() {
    if (!deletingApplication) return;
    setDeleteBusy(true);
    setDeleteError(null);
    try {
      await deleteApplication(deletingApplication.id);
      removeApplication(deletingApplication.id);
      toastSuccess(t("applications.removedToast", { name: deletingApplication.name }));
      setDeletingApplication(null);
    } catch (err) {
      setDeleteError(err instanceof Error ? err.message : t("applications.couldntRemove"));
    } finally {
      setDeleteBusy(false);
    }
  }

  return (
    <div className="page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">{t("applications.title")}</h1>
          <p className="page-subtitle">{t("applications.subtitle")}</p>
        </div>
        <Button onClick={() => setWizardOpen(true)}>
          <Icon name="plus" size={16} />
          {t("applications.create")}
        </Button>
      </div>

      {actionError && <p className="page-error-note">{actionError}</p>}

      {applications.length === 0 ? (
        <Card>
          <EmptyState icon="box" title={t("applications.emptyTitle")} description={t("applications.emptyDescription")} />
        </Card>
      ) : (
        <div className="applications-grid">
          {applications.map((application) => (
            <ApplicationCard
              key={application.id}
              application={application}
              serverName={servers.find((s) => s.id === application.serverId)?.name}
              busy={busyId === application.id}
              onOpen={() => navigate(`/applications/${application.id}`)}
              onStart={() => runAction(application.id, () => startApplication(application.id))}
              onStop={() => runAction(application.id, () => stopApplication(application.id, true))}
              onRestart={() => runAction(application.id, () => restartApplication(application.id))}
              onKill={() => runAction(application.id, () => killApplication(application.id))}
              onDelete={() => {
                setDeleteError(null);
                setDeletingApplication(application);
              }}
            />
          ))}
        </div>
      )}

      {wizardOpen && (
        <CreateApplicationWizard
          onClose={() => setWizardOpen(false)}
          onCreated={() => {
            setWizardOpen(false);
            reload();
          }}
        />
      )}

      {deletingApplication && (
        <DeleteApplicationDialog
          applicationName={deletingApplication.name}
          busy={deleteBusy}
          error={deleteError}
          onConfirm={handleConfirmDelete}
          onCancel={() => setDeletingApplication(null)}
        />
      )}
    </div>
  );
}
