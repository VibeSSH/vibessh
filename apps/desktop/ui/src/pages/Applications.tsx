import { useEffect, useMemo, useState } from "react";
import { DndContext, closestCenter, useSensor, useSensors, type DragEndEvent } from "@dnd-kit/core";
import { SortableContext, arrayMove, rectSortingStrategy } from "@dnd-kit/sortable";
import { CardPointerSensor, SortableApplicationCard } from "@/components/applications/SortableApplicationCard";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { Select } from "@/components/ui/Select";
import { ApplicationCard } from "@/components/applications/ApplicationCard";
import { ApplicationTabs } from "@/components/applications/ApplicationTabs";
import { CreateApplicationWizard } from "@/components/applications/CreateApplicationWizard";
import { AdoptServersModal } from "@/components/applications/AdoptServersModal";
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
import { listSharedApplicationAccess, syncSharedApplications, type SharedApplicationAccess } from "@/services/cloudService";
import { APPLICATIONS_LIFECYCLE } from "@/constants/permissions";
import { useApplicationsStore } from "@/stores/applicationsStore";
import { useServersStore } from "@/stores/serversStore";
import { toastError, toastSuccess } from "@/stores/toastStore";
import type { Application, Blueprint, RuntimeType } from "@/types/application";
import "./pages.css";
import "./Applications.css";
import "@/components/servers/forms.css";
import { errorMessage } from "@/services/tauri";

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
const ORDER_STORAGE_KEY = "vibessh.applications.order";

/**
 * How long a card has to be held before it is picked up, and how far the
 * pointer may drift in that time.
 *
 * The card already answers a plain click by selecting itself, so a drag
 * cannot start on press alone. The tolerance is what keeps a hand that is not
 * perfectly still from cancelling the hold.
 */
const DRAG_HOLD_MS = 220;
const DRAG_TOLERANCE_PX = 6;
const RUNTIME_TYPE_ORDER: RuntimeType[] = ["docker", "systemd", "remoteProcess", "localProcess"];

/**
 * The order the user dragged their Applications into, as a list of ids.
 *
 * Kept on this machine rather than in the database: it is a preference about
 * how one person likes to look at their own screen, not a property of the
 * Applications themselves, and two people sharing a team should not be
 * rearranging each other's grids.
 */
function loadOrder(): string[] {
  try {
    const stored = localStorage.getItem(ORDER_STORAGE_KEY);
    if (!stored) return [];
    const parsed: unknown = JSON.parse(stored);
    return Array.isArray(parsed) ? parsed.filter((id): id is string => typeof id === "string") : [];
  } catch {
    // Unreadable or not JSON - the natural order is a fine fallback, and
    // there is nothing here worth reporting to anybody.
    return [];
  }
}

function saveOrder(order: string[]) {
  try {
    localStorage.setItem(ORDER_STORAGE_KEY, JSON.stringify(order));
  } catch {
    // Storage unavailable. The order still holds for this session; it just
    // will not survive a restart, which is not worth an error toast.
  }
}

/**
 * Applies the saved order, putting anything it does not mention at the end.
 *
 * New Applications appear last rather than in the middle, and one deleted
 * elsewhere simply stops matching - so a stale saved order degrades into a
 * partial one instead of hiding or duplicating anything.
 */
function applyOrder(applications: Application[], order: string[]): Application[] {
  if (order.length === 0) return applications;
  const rank = new Map(order.map((id, index) => [id, index]));
  return [...applications].sort((a, b) => (rank.get(a.id) ?? Number.MAX_SAFE_INTEGER) - (rank.get(b.id) ?? Number.MAX_SAFE_INTEGER));
}

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
  const [adoptOpen, setAdoptOpen] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [deletingApplication, setDeletingApplication] = useState<Application | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [bulkOpen, setBulkOpen] = useState(false);
  const [removeFiles, setRemoveFiles] = useState(false);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [blueprints, setBlueprints] = useState<Blueprint[]>([]);
  const [groupBy, setGroupBy] = useState<GroupBy>(loadGroupBy);
  const [order, setOrder] = useState<string[]>(loadOrder);
  const sensors = useSensors(useSensor(CardPointerSensor, { activationConstraint: { delay: DRAG_HOLD_MS, tolerance: DRAG_TOLERANCE_PX } }));

  /** Which listed applications are somebody else's, shared with this account. */
  const [sharedAccess, setSharedAccess] = useState<Map<string, SharedApplicationAccess>>(new Map());

  function reload() {
    listApplications()
      .then(setApplications)
      .catch(() => {
        // No applications yet, or this loaded outside a Tauri webview during development.
      });
    listSharedApplicationAccess()
      .then((rows) => setSharedAccess(new Map(rows.map((row) => [row.applicationId, row]))))
      .catch(() => setSharedAccess(new Map()));
  }

  useEffect(() => {
    reload();
    // What teammates shared with this account, brought up to date and then
    // listed. After the first reload rather than before it, so the list the
    // person already has is on screen while the backend is asked.
    syncSharedApplications()
      .then((report) => {
        if (report.added > 0 || report.removed > 0 || report.refreshed > 0) reload();
      })
      .catch((err) => console.warn("couldn't sync shared applications", err));
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

  const ordered = useMemo(() => applyOrder(applications, order), [applications, order]);
  const groups = useMemo(() => groupApplications(ordered, groupBy, blueprints, servers, t), [ordered, groupBy, blueprints, servers, t]);

  /**
   * Moves a card, writing the result back as a full ordering.
   *
   * The saved list covers every Application, not just the group that was
   * dragged in - otherwise the ones in other groups would have no rank and
   * would all pile up at the end the moment anything moved.
   */
  function handleDragEnd(event: DragEndEvent) {
    const { active, over } = event;
    if (!over || active.id === over.id) return;
    const ids = ordered.map((application) => application.id);
    const from = ids.indexOf(String(active.id));
    const to = ids.indexOf(String(over.id));
    if (from === -1 || to === -1) return;
    const next = arrayMove(ids, from, to);
    setOrder(next);
    saveOrder(next);
  }

  async function runAction(id: string, action: () => Promise<unknown>) {
    setActionError(null);
    setBusyId(id);
    try {
      await action();
      reload();
    } catch (err) {
      setActionError(errorMessage(err, t));
    } finally {
      setBusyId(null);
    }
  }

  async function handleConfirmDelete() {
    if (!deletingApplication) return;
    setDeleteBusy(true);
    setDeleteError(null);
    try {
      const report = await deleteApplication(deletingApplication.id, removeFiles);
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
      setDeleteError(errorMessage(err, t));
    } finally {
      setDeleteBusy(false);
    }
  }

  function toggleSelected(id: string, isSelected: boolean) {
    setSelected((current) => {
      const next = new Set(current);
      if (isSelected) next.add(id);
      else next.delete(id);
      return next;
    });
  }

  /**
   * Removes every selected Application, one after another.
   *
   * Sequential rather than parallel: each teardown talks to the same Node
   * over the same SSH session, destroys a container and revokes firewall
   * rules, and ten of those at once is a good way to get a half-applied
   * firewall. Slower and legible beats fast and interleaved.
   *
   * One failure does not stop the rest - the others are still removable, and
   * stopping halfway would leave a selection nobody can reason about.
   */
  async function handleConfirmBulkDelete() {
    setDeleteBusy(true);
    setDeleteError(null);
    const failures: string[] = [];
    for (const id of selected) {
      const application = applications.find((candidate) => candidate.id === id);
      try {
        await deleteApplication(id, removeFiles);
        removeApplication(id);
      } catch (err) {
        failures.push(`${application?.name ?? id}: ${errorMessage(err, t)}`);
      }
    }
    setDeleteBusy(false);
    if (failures.length > 0) {
      setDeleteError(failures.join("; "));
      toastError(t("applications.bulkRemovedWithFailuresToast", { count: failures.length }));
      return;
    }
    toastSuccess(t("applications.bulkRemovedToast", { count: selected.size }));
    setSelected(new Set());
    setBulkOpen(false);
    reload();
  }

  const selectedNames = applications
    .filter((application) => selected.has(application.id))
    .map((application) => application.name)
    .join(", ");

  return (
    <div className="page">
      {/* Also here, not only inside an Application: this is the page people
          land on, and the strip is what stops them walking the list again to
          get back to what they were doing. */}
      <ApplicationTabs />
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">{t("applications.title")}</h1>
          <p className="page-subtitle">{t("applications.subtitle")}</p>
        </div>
        <div className="applications-header-actions">
          {/* Beside "create" rather than inside the wizard: adopting a
              server that exists and setting one up from nothing are different
              intentions, and somebody with a migrated host arrives holding
              the first one. */}
          <Button variant="secondary" onClick={() => setAdoptOpen(true)}>
            <Icon name="search" size={16} />
            {t("applications.adopt")}
          </Button>
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
        <>
        {/* Grouping is a way of looking at the list, not a header action, so
            it sits in a toolbar above the grid rather than crowding the
            create/adopt buttons in the header. */}
        <div className="applications-toolbar">
          <label className="applications-group-by">
            <span className="form-label">{t("applications.groupByLabel")}</span>
            <Select
              value={groupBy}
              onChange={(value) => handleGroupByChange(value as GroupBy)}
              items={[
                { value: "none", label: t("applications.groupByNone") },
                { value: "egg", label: t("applications.groupByEgg") },
                { value: "runtimeType", label: t("applications.groupByRuntimeType") },
                { value: "server", label: t("applications.groupByServer") },
              ]}
            />
          </label>
        </div>
        {groups.map((group) => (
          <section key={group.key} className="applications-group">
            {group.label && <h2 className="applications-group-title">{group.label}</h2>}
            {/* A context per group, so a card cannot be dragged into another
                one. The groups are a way of looking at the list - by Node, by
                image - not somewhere an Application can be moved to. */}
            <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
              <SortableContext items={group.applications.map((application) => application.id)} strategy={rectSortingStrategy}>
                <div className="applications-grid">
                  {group.applications.map((application) => (
                    <SortableApplicationCard key={application.id} id={application.id}>
                <ApplicationCard
                  application={application}
                  serverName={servers.find((s) => s.id === application.serverId)?.name}
                  busy={busyId === application.id}
                  selected={sharedAccess.has(application.id) ? undefined : selected.has(application.id)}
                  onSelectedChange={sharedAccess.has(application.id) ? undefined : (isSelected) => toggleSelected(application.id, isSelected)}
                  shared={
                    sharedAccess.has(application.id)
                      ? { canLifecycle: sharedAccess.get(application.id)?.permissions.includes(APPLICATIONS_LIFECYCLE) ?? false }
                      : null
                  }
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
                    </SortableApplicationCard>
                  ))}
                </div>
              </SortableContext>
            </DndContext>
          </section>
        ))}
        </>
      )}

      {adoptOpen && (
        <AdoptServersModal
          servers={servers}
          onClose={() => setAdoptOpen(false)}
          onAdopted={reload}
        />
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

      {/* Only present once something is selected: a bar offering to delete
          nothing is a bar in the way. */}
      {selected.size > 0 && (
        <div className="applications-bulk-bar">
          <span className="applications-bulk-count">{t("applications.selectedCount", { count: selected.size })}</span>
          <Button variant="secondary" size="sm" onClick={() => setSelected(new Set())}>
            {t("applications.clearSelection")}
          </Button>
          <Button
            variant="danger"
            size="sm"
            onClick={() => {
              setDeleteError(null);
              setBulkOpen(true);
            }}
          >
            <Icon name="trash" size={14} />
            {t("applications.removeSelected", { count: selected.size })}
          </Button>
        </div>
      )}

      {deletingApplication && (
        <DeleteApplicationDialog
          applicationName={deletingApplication.name}
          busy={deleteBusy}
          error={deleteError}
          removeFiles={removeFiles}
          onRemoveFilesChange={setRemoveFiles}
          onConfirm={handleConfirmDelete}
          onCancel={() => setDeletingApplication(null)}
        />
      )}

      {bulkOpen && (
        <DeleteApplicationDialog
          applicationName={selectedNames}
          count={selected.size}
          busy={deleteBusy}
          error={deleteError}
          removeFiles={removeFiles}
          onRemoveFilesChange={setRemoveFiles}
          onConfirm={handleConfirmBulkDelete}
          onCancel={() => setBulkOpen(false)}
        />
      )}
    </div>
  );
}
