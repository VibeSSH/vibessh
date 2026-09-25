import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ConnectionsCard } from "@/components/applications/ConnectionsCard";
import { queryKeys } from "@/services/queryKeys";
import { getNodeFirewallOverview } from "@/services/serverService";
import { portProtection, type PortProtection } from "./portProtection";
import { GuideLink } from "@/guide/GuideLink";
import { useCanOnServer } from "@/stores/nodePermissionsStore";
import { Badge } from "@/components/ui/Badge";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { Select } from "@/components/ui/Select";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { useModalDialog } from "@/hooks/useModalDialog";
import {
  addApplicationPort,
  listApplicationPorts,
  removeApplicationPort,
  syncApplicationNodeFirewall,
  updateApplicationPort,
  type FirewallSyncResult,
} from "@/services/applicationService";
import { useContainerApply } from "@/hooks/useContainerApply";
import type { ApplicationDetail, ApplicationPort, PortInput, PortProtocol, PortVisibility } from "@/types/application";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";
import "./PortsTab.css";
import { errorMessage } from "@/services/tauri";

/**
 * Says whether one port is actually restricted right now.
 *
 * Nothing at all for a port that is meant to be public - a red badge on
 * every correctly-published port is how people learn to ignore red badges -
 * and nothing while the answer is unknown, because a confident "protected"
 * there would be the dangerous guess.
 */
function ProtectionBadge({ state }: { state: PortProtection }) {
  const { t } = useTranslation();
  if (state === "public" || state === "unknown") return null;
  return (
    <Badge tone={state === "protected" ? "success" : "danger"}>
      {t(state === "protected" ? "portsTab.protected" : "portsTab.unprotected")}
    </Badge>
  );
}

interface PortsTabProps {
  applicationId: string;
  application: ApplicationDetail;
}

/**
 * How exposed a port is, in the badge's own colour.
 *
 * The list showed every visibility as the same neutral chip, so "reachable
 * from the whole internet" and "reachable from this host only" looked
 * identical in the one place somebody scans to check exactly that. Public is
 * a warning rather than a danger: publishing a port is a normal thing to
 * want, it is just the choice that deserves a second look.
 */
function visibilityTone(visibility: PortVisibility): "neutral" | "success" | "warning" {
  if (visibility === "public") return "warning";
  if (visibility === "vibeNetwork") return "success";
  return "neutral";
}

/** Declared ports are documentation of intent, not a live guarantee - see applicationService.listApplicationPorts's own doc comment. A "required" port (blueprint-declared, none of the built-in blueprints set one up yet) can be edited but not removed here, same rule the backend itself enforces. */
export function PortsTab({ applicationId, application }: PortsTabProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const applyToContainer = useContainerApply();
  // A member without this sees the ports and cannot change them - the
  // list is the useful half and reading it harms nothing.
  const canEditPorts = useCanOnServer(application.serverId, "applications.ports");
  // Cached, so coming back to this tab paints the list it had rather than a
  // skeleton over a fresh round trip. `isPending` is only the first load:
  // a background refresh leaves the rows on screen.
  const {
    data: ports = [],
    isPending: loading,
    error: loadError,
  } = useQuery({
    queryKey: queryKeys.applicationPorts(applicationId),
    queryFn: () => listApplicationPorts(applicationId),
  });
  const error = loadError ? errorMessage(loadError, t) : null;

  /**
   * The Node's firewall, read on open rather than after a Sync press.
   *
   * A published Docker port is bound widely and only this narrows it, so
   * "is this port actually restricted" is the question somebody arrives at
   * the tab with - not one they should have to press a button that sounds
   * like it changes something to answer. A local application has no Node,
   * and a failed read stays `undefined` rather than becoming "unprotected":
   * not knowing is its own answer, and the badge says so.
   */
  const { data: firewall } = useQuery({
    queryKey: queryKeys.nodeFirewall(application.serverId ?? ""),
    queryFn: () => getNodeFirewallOverview(application.serverId as string),
    enabled: Boolean(application.serverId),
    retry: false,
  });

  const [formOpen, setFormOpen] = useState(false);
  const [editingPort, setEditingPort] = useState<ApplicationPort | null>(null);
  const [deletingPort, setDeletingPort] = useState<ApplicationPort | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const deleteBackdrop = useModalDialog(() => !deleteBusy && setDeletingPort(null), { labelledBy: "portstab-dialog-title-1" });

  const [firewallSyncing, setFirewallSyncing] = useState(false);
  // `undefined` = never synced this session yet; `null` = synced, but this
  // is a Local application (no Node to sync a firewall against).
  const [firewallResult, setFirewallResult] = useState<FirewallSyncResult | null | undefined>(undefined);
  const [firewallError, setFirewallError] = useState<string | null>(null);

  async function handleSyncFirewall() {
    setFirewallSyncing(true);
    setFirewallError(null);
    try {
      setFirewallResult(await syncApplicationNodeFirewall(applicationId));
    } catch (err) {
      setFirewallError(errorMessage(err, t));
    } finally {
      setFirewallSyncing(false);
    }
  }

  /** The ports list is kept current by writing the mutation's own result into
   * the cache, so a change shows at once with no second SSH refetch. */
  const upsertPortInCache = (port: ApplicationPort) => {
    queryClient.setQueryData<ApplicationPort[]>(queryKeys.applicationPorts(applicationId), (old = []) =>
      old.some((existing) => existing.id === port.id) ? old.map((existing) => (existing.id === port.id ? port : existing)) : [...old, port],
    );
  };
  const removePortFromCache = (portId: string) => {
    queryClient.setQueryData<ApplicationPort[]>(queryKeys.applicationPorts(applicationId), (old = []) => old.filter((existing) => existing.id !== portId));
  };

  async function handleConfirmDelete() {
    if (!deletingPort) return;
    setDeleteBusy(true);
    setDeleteError(null);
    try {
      await removeApplicationPort(applicationId, deletingPort.id);
      removePortFromCache(deletingPort.id);
      setDeletingPort(null);
      // The published `-p` only leaves the container on a recreate; run it in
      // the background so the row disappears at once.
      void applyToContainer(application);
    } catch (err) {
      setDeleteError(errorMessage(err, t));
    } finally {
      setDeleteBusy(false);
    }
  }

  return (
    <div className="application-detail-overview">
      {error && <p className="page-error-note">{error}</p>}

      <Card
        title={t("portsTab.title")}
        subtitle={t("portsTab.description")}
        actions={
          <>
            {/* Next to the feature, not buried in a menu: somebody who does
                not know what "Vibe Network only" means is looking at the
                thing, not searching for its name. */}
            <GuideLink topic="ports" />
            {canEditPorts && (
              <Button
                size="sm"
                onClick={() => {
                  setEditingPort(null);
                  setFormOpen(true);
                }}
              >
                <Icon name="plus" size={14} />
                {t("portsTab.addPort")}
              </Button>
            )}
          </>
        }
      >

        {loading ? (
          <SkeletonRows />
        ) : ports.length === 0 ? (
          <EmptyState icon="wifi" title={t("portsTab.emptyTitle")} description={t("portsTab.emptyDescription")} />
        ) : (
          <>
          {/* A table rather than a line of prose per port: the protocol, the
              two port numbers, who can reach it and whether the firewall
              agrees each get a column, so a server with a dozen ports can be
              read down a column instead of parsed row by row. */}
          <div className="port-row port-row-head" role="presentation">
            <span className="port-head-cell">{t("portsTab.columnName")}</span>
            <span className="port-head-cell">{t("portsTab.columnProtocol")}</span>
            <span className="port-head-cell">{t("portsTab.columnInternal")}</span>
            <span className="port-head-cell">{t("portsTab.columnExternal")}</span>
            <span className="port-head-cell">{t("portsTab.columnAccess")}</span>
            <span className="port-head-cell">{t("portsTab.columnProtection")}</span>
            <span />
          </div>
          <ul className="port-rows">
            {ports.map((port) => (
              <li key={port.id} className="port-row">
                <span className="port-name" title={port.name}>
                  <span className="port-name-text">{port.name}</span>
                  {port.required && <span className="port-required">{t("portsTab.required")}</span>}
                </span>
                <span className="port-protocol">{port.protocol.toUpperCase()}</span>
                <span className="port-cell port-mono">
                  {port.bindAddress}:{port.internalPort}
                </span>
                <span className="port-cell port-mono">{port.externalPort ?? "—"}</span>
                <span>
                  <Badge tone={visibilityTone(port.visibility)}>{t(`applicationNetwork.visibility.${port.visibility}`)}</Badge>
                </span>
                <span>
                  <ProtectionBadge state={portProtection(port, firewall)} />
                </span>
                <span className="port-row-actions">
                {canEditPorts && (
                <IconButton
                  icon="edit"
                  size="sm"
                  title={t("portsTab.editAria", { name: port.name })}
                  onClick={() => {
                    setEditingPort(port);
                    setFormOpen(true);
                  }}
                />
                )}
                {!port.required && canEditPorts && (
                  <IconButton
                    icon="trash"
                    size="sm"
                    danger
                    title={t("portsTab.deleteAria", { name: port.name })}
                    onClick={() => {
                      setDeleteError(null);
                      setDeletingPort(port);
                    }}
                  />
                )}
                </span>
              </li>
            ))}
          </ul>
          </>
        )}

        <div className="ports-firewall">
          <div className="ports-firewall-text">
            <span className="ports-firewall-title">{t("portsTab.firewallTitle")}</span>
            <p className="form-note ports-firewall-status">
              {firewallResult === undefined && t("portsTab.firewallSyncNote")}
              {firewallResult === null && t("portsTab.firewallSyncLocal")}
              {firewallResult && firewallResult.backend === null && (
                <span className="form-note-danger">{t("portsTab.firewallSyncNoBackend")}</span>
              )}
              {firewallResult &&
                firewallResult.backend !== null &&
                t(firewallResult.rulesRemoved > 0 ? "portsTab.firewallSyncSummaryWithRemoved" : "portsTab.firewallSyncSummary", {
                  backend: firewallResult.backend,
                  status: firewallResult.active ? t("portsTab.firewallActive") : t("portsTab.firewallInactive"),
                  count: firewallResult.rulesApplied,
                  removed: firewallResult.rulesRemoved,
                })}
              {/* A sync that "succeeded" while nothing enforces the rules is the
                  case that made "Vibe Network only" ports publicly reachable -
                  it has to read as a warning, not as part of the summary. */}
              {firewallResult && firewallResult.backend !== null && firewallResult.unenforced && (
                <span className="form-note-danger"> {t("portsTab.firewallUnenforced")}</span>
              )}
              {firewallError && <span className="form-note-danger"> {firewallError}</span>}
            </p>
          </div>
          <Button variant="secondary" size="sm" onClick={handleSyncFirewall} disabled={firewallSyncing || !canEditPorts}>
            <Icon name="lock" size={14} />
            {firewallSyncing ? t("common.saving") : t("portsTab.syncFirewall")}
          </Button>
        </div>
      </Card>

      {/* Ports are who can reach this application from outside the node;
          connections are who can reach it from inside it. They belong on the
          same tab because until this existed only the first half was visible,
          and the second half was "everything". */}
      <ConnectionsCard application={application} />

      {formOpen && (
        <PortFormModal
          applicationId={applicationId}
          editingPort={editingPort}
          onClose={() => setFormOpen(false)}
          onSaved={(port) => {
            setFormOpen(false);
            upsertPortInCache(port);
            void applyToContainer(application);
          }}
        />
      )}

      {deletingPort && (
        <div className="modal-backdrop" {...deleteBackdrop.backdropProps}>
          <div className="modal-panel modal-panel-sm" {...deleteBackdrop.panelProps}>
            <div className="modal-header">
              <h2 className="modal-title" id="portstab-dialog-title-1">{t("portsTab.deleteTitle")}</h2>
              <IconButton icon="x" size="sm" onClick={() => setDeletingPort(null)} title={t("common.close")} />
            </div>
            <div className="modal-body">
              <p className="dialog-body-text">{t("portsTab.deleteBody", { name: deletingPort.name })}</p>
              {deleteError && <p className="form-note form-note-danger form-note-spaced">{deleteError}</p>}
              <div className="form-actions">
                <Button variant="secondary" onClick={() => setDeletingPort(null)} disabled={deleteBusy}>
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

interface PortFormModalProps {
  applicationId: string;
  editingPort: ApplicationPort | null;
  onClose: () => void;
  onSaved: (port: ApplicationPort) => void;
}

function PortFormModal({ applicationId, editingPort, onClose, onSaved }: PortFormModalProps) {
  const { t } = useTranslation();
  const backdrop = useModalDialog(onClose, { labelledBy: "portstab-dialog-title-2" });
  const isEditing = Boolean(editingPort);

  const [name, setName] = useState(editingPort?.name ?? "");
  const [protocol, setProtocol] = useState<PortProtocol>(editingPort?.protocol ?? "tcp");
  const [visibility, setVisibility] = useState<PortVisibility>(editingPort?.visibility ?? "public");
  const [customAddress, setCustomAddress] = useState(editingPort?.visibility === "custom" ? editingPort.bindAddress : "");
  const [internalPort, setInternalPort] = useState(editingPort ? String(editingPort.internalPort) : "");
  const [externalPort, setExternalPort] = useState(editingPort?.externalPort ? String(editingPort.externalPort) : "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const internal = Number(internalPort);
    if (!name.trim() || !Number.isInteger(internal) || internal < 1 || internal > 65535) {
      setError(t("portsTab.invalidForm"));
      return;
    }
    const external = externalPort.trim() ? Number(externalPort) : undefined;

    const input: PortInput = {
      name: name.trim(),
      protocol,
      bindAddress: visibility === "custom" ? customAddress.trim() || "0.0.0.0" : "0.0.0.0",
      internalPort: internal,
      externalPort: external,
      visibility,
    };

    setBusy(true);
    setError(null);
    try {
      // The mutation returns the saved port; hand it back so the list updates
      // from it directly. The container recreate the `-p` change needs runs in
      // the background from the parent, not awaited here.
      const saved = editingPort ? await updateApplicationPort(applicationId, editingPort.id, input) : await addApplicationPort(applicationId, input);
      onSaved(saved);
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" {...backdrop.backdropProps}>
      <div className="modal-panel" {...backdrop.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="portstab-dialog-title-2">{isEditing ? t("portsTab.editTitle") : t("portsTab.addTitle")}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <form className="server-form" onSubmit={handleSubmit}>
          <div className="modal-body">
            {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
            <label className="form-field">
              <span className="form-label">{t("portsTab.name")}</span>
              <input className="form-input" value={name} onChange={(e) => setName(e.target.value)} placeholder={t("portsTab.namePlaceholder")} />
            </label>
            <div className="form-row">
              <label className="form-field form-field-narrow">
                <span className="form-label">{t("portsTab.protocol")}</span>
                <Select
                  value={protocol}
                  onChange={(value) => setProtocol(value as PortProtocol)}
                  items={[
                    { value: "tcp", label: "TCP" },
                    { value: "udp", label: "UDP" },
                  ]}
                />
              </label>
              <label className="form-field form-field-grow">
                <span className="form-label">{t("applicationNetwork.access")}</span>
                <Select
                  value={visibility}
                  onChange={(value) => setVisibility(value as PortVisibility)}
                  items={[
                    { value: "public", label: t("applicationNetwork.visibility.public") },
                    { value: "vibeNetwork", label: t("applicationNetwork.visibility.vibeNetwork") },
                    { value: "localhost", label: t("applicationNetwork.visibility.localhost") },
                    { value: "custom", label: t("applicationNetwork.visibility.custom") },
                  ]}
                />
              </label>
            </div>
            <p className="form-note">{t(`applicationNetwork.visibilityHelp.${visibility}`)}</p>
            {visibility === "custom" && (
              <label className="form-field">
                <span className="form-label">{t("portsTab.bindAddress")}</span>
                <input className="form-input" value={customAddress} onChange={(e) => setCustomAddress(e.target.value)} placeholder="0.0.0.0" />
              </label>
            )}
            <div className="form-row">
              <label className="form-field">
                <span className="form-label">{t("portsTab.internalPort")}</span>
                <input
                  className="form-input"
                  type="number"
                  min={1}
                  max={65535}
                  value={internalPort}
                  onChange={(e) => setInternalPort(e.target.value)}
                  placeholder="25565"
                />
              </label>
              <label className="form-field">
                <span className="form-label">{t("portsTab.externalPort")}</span>
                <input
                  className="form-input"
                  type="number"
                  min={1}
                  max={65535}
                  value={externalPort}
                  onChange={(e) => setExternalPort(e.target.value)}
                  placeholder={t("portsTab.externalPortPlaceholder")}
                />
              </label>
            </div>
            <div className="form-actions">
              <Button type="button" variant="secondary" onClick={onClose} disabled={busy}>
                {t("common.cancel")}
              </Button>
              <Button type="submit" disabled={busy}>
                {busy ? t("common.saving") : t("common.save")}
              </Button>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}
