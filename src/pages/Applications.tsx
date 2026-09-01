import { useEffect, useMemo, useState } from "react";
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
  listBlueprints,
  restartApplication,
  startApplication,
  stopApplication,
} from "@/services/applicationService";
import { listServers, serverSummaryToManagedServer } from "@/services/serverService";
import { useApplicationsStore } from "@/stores/applicationsStore";
import { useServersStore } from "@/stores/serversStore";
import { toastError, toastSuccess } from "@/stores/toastStore";
import type { Application, Blueprint, RuntimeType } from "@/types/application";
import "./pages.css";
import "./Applications.css";
import "@/components/servers/forms.css";

/**
 * Grouping is a pure display concern - which existing field partitions the
 * list, remembered as a per-device preference (`localStorage`, never sent
 * anywhere). It never changes what any Application actually *is*: no new
 * entity, no server-side field, nothing that routing/firewall/Vibe Network/
 * DNS/runtime/permissions/deployment could ever read - moving an
 * Application between groups is just re-rendering the same list differently,
 * not a mutation.
 */
type GroupBy = "none" | "egg" | "runtimeType" | "server";
const GROUP_BY_STORAGE_KEY = "vibessh.applications.groupBy";
const RUNTIME_TYPE_ORDER: RuntimeType[] = ["docker", "systemd", "remoteProcess", "localProcess"];

function loadGroupBy(): GroupBy {
  try {
    const stored = localStorage.getItem(GROUP_BY_STORAGE_KEY);
    if (stored === "egg" || stored === "runtimeType" || stored === "server") return stored;
  } catch {
    // localStorage unavailable (private browsing, disabled site data) - the
    // ungrouped default is a perfectly fine fallback, not an error.
  }
  return "none";
}

interface ApplicationGroup {
  key: string;
  label: string;
  applications: Application[];
}

function groupApplications(
  applications: Application[],
  groupBy: GroupBy,
  blueprints: Blueprint[],
  servers: { id: string; name: string }[],
  t: (key: string, options?: Record<string, unknown>) => string,
): ApplicationGroup[] {
  if (groupBy === "none") {
    return [{ key: "all", label: "", applications }];
  }

  const buckets = new Map<string, ApplicationGroup>();
  const orderedKeys: string[] = [];

  function bucket(key: string, label: string): ApplicationGroup {
    let group = buckets.get(key);
    if (!group) {
      group = { key, label, applications: [] };
      buckets.set(key, group);
      orderedKeys.push(key);
    }
    return group;
  }

  if (groupBy === "runtimeType") {
    for (const type of RUNTIME_TYPE_ORDER) {
      bucket(type, t(`createApplicationWizard.runtimeTypeOption.${type}`));
    }
  }

  for (const application of applications) {
    let group: ApplicationGroup;
    if (groupBy === "egg") {
      const blueprint = blueprints.find((b) => b.id === application.blueprintId);
      group = bucket(application.blueprintId, blueprint?.name ?? t("applications.groupOther"));
    } else if (groupBy === "runtimeType") {
      group = bucket(application.runtimeType, t(`createApplicationWizard.runtimeTypeOption.${application.runtimeType}`));
    } else {
      const server = application.serverId ? servers.find((s) => s.id === application.serverId) : undefined;
      const key = application.serverId ?? "local";
      group = bucket(key, server?.name ?? t("applicationCard.local"));
    }
    group.applications.push(application);
  }

  return orderedKeys.map((key) => buckets.get(key)!).filter((group) => group.applications.length > 0);
}

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
  const [blueprints, setBlueprints] = useState<Blueprint[]>([]);
  const [groupBy, setGroupBy] = useState<GroupBy>(loadGroupBy);

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
    listBlueprints()
      .then(setBlueprints)
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function handleGroupByChange(next: GroupBy) {
    setGroupBy(next);
    try {
      localStorage.setItem(GROUP_BY_STORAGE_KEY, next);
    } catch {
      // Best-effort - the choice just won't survive a reload, no worse than not persisting it at all.
    }
  }

  const groups = useMemo(() => groupApplications(applications, groupBy, blueprints, servers, t), [applications, groupBy, blueprints, servers, t]);

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
      const report = await deleteApplication(deletingApplication.id);
      removeApplication(deletingApplication.id);
      // The row is gone either way, but a partial teardown leaves the
      // container running and still holding its published port - reporting
      // that as a plain success is what made a replacement Application fail
      // later with an unexplained "port is already allocated".
      if (report.warnings.length > 0) {
        toastError(t("applications.removedWithWarningsToast", { name: deletingApplication.name, warning: report.warnings[0] }));
      } else {
        toastSuccess(t("applications.removedToast", { name: deletingApplication.name }));
      }
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
        <div className="applications-header-actions">
          {applications.length > 0 && (
            <label className="applications-group-by">
              <span className="form-label">{t("applications.groupByLabel")}</span>
              <select className="form-input" value={groupBy} onChange={(e) => handleGroupByChange(e.target.value as GroupBy)}>
                <option value="none">{t("applications.groupByNone")}</option>
                <option value="egg">{t("applications.groupByEgg")}</option>
                <option value="runtimeType">{t("applications.groupByRuntimeType")}</option>
                <option value="server">{t("applications.groupByServer")}</option>
              </select>
            </label>
          )}
          <Button onClick={() => setWizardOpen(true)}>
            <Icon name="plus" size={16} />
            {t("applications.create")}
          </Button>
        </div>
      </div>

      {actionError && <p className="page-error-note">{actionError}</p>}

      {applications.length === 0 ? (
        <Card>
          <EmptyState icon="box" title={t("applications.emptyTitle")} description={t("applications.emptyDescription")} />
        </Card>
      ) : (
        groups.map((group) => (
          <section key={group.key} className="applications-group">
            {group.label && <h2 className="applications-group-title">{group.label}</h2>}
            <div className="applications-grid">
              {group.applications.map((application) => (
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
          </section>
        ))
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
