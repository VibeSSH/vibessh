import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { useModalDialog } from "@/hooks/useModalDialog";
import {
  addApplicationPort,
  listApplicationPorts,
  recreateApplication,
  refreshApplicationStatus,
  removeApplicationPort,
  syncApplicationNodeFirewall,
  updateApplicationPort,
  type FirewallSyncResult,
} from "@/services/applicationService";
import type { ApplicationDetail, ApplicationPort, PortInput, PortProtocol, PortVisibility } from "@/types/application";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";
import { errorMessage } from "@/services/tauri";

interface PortsTabProps {
  applicationId: string;
  application: ApplicationDetail;
}

/** A Docker container's published ports (`-p host:container`) are baked in
 * at `docker create` time (see `runtime::docker`'s own doc comment) - a
 * plain restart reuses the same, now-stale container, so adding/editing/
 * removing a port would silently never actually take effect on a running
 * app. Same Pterodactyl-matching "change it, it just works" auto-recreate
 * `ApplicationConfigCard`/`ResourceLimitsCard` already do for their own
 * saves - a stopped app is left stopped, see those components' own doc
 * comments for why auto-starting it would be its own surprise. Best-effort:
 * a failed recreate here doesn't undo the port change that already saved
 * successfully, it just means the user needs to notice and recreate by hand.
 *
 * Checks the freshly-probed status, not the `application.status` prop - see
 * `EnvironmentTab`'s own copy of this function for why that prop alone
 * isn't trustworthy enough for this check. */
async function recreateIfRunningDocker(application: ApplicationDetail) {
  if (application.runtimeType !== "docker") return;
  const status = await refreshApplicationStatus(application.id);
  if (status === "running") {
    await recreateApplication(application.id);
  }
}

/** Declared ports are documentation of intent, not a live guarantee - see applicationService.listApplicationPorts's own doc comment. A "required" port (blueprint-declared, none of the built-in blueprints set one up yet) can be edited but not removed here, same rule the backend itself enforces. */
export function PortsTab({ applicationId, application }: PortsTabProps) {
  const { t } = useTranslation();
  const [ports, setPorts] = useState<ApplicationPort[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

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

  const reload = useCallback(() => {
    setLoading(true);
    setError(null);
    listApplicationPorts(applicationId)
      .then(setPorts)
      .catch((err) => setError(errorMessage(err, t)))
      .finally(() => setLoading(false));
  }, [applicationId, t]);

  useEffect(reload, [reload]);

  async function handleConfirmDelete() {
    if (!deletingPort) return;
    setDeleteBusy(true);
    setDeleteError(null);
    try {
      await removeApplicationPort(applicationId, deletingPort.id);
      setDeletingPort(null);
      await recreateIfRunningDocker(application);
      reload();
    } catch (err) {
      setDeleteError(errorMessage(err, t));
    } finally {
      setDeleteBusy(false);
    }
  }

  return (
    <div className="application-detail-overview">
      {error && <p className="page-error-note">{error}</p>}

      <div className="application-detail-header-row">
        <p className="form-note">{t("portsTab.description")}</p>
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
      </div>

      <div className="application-detail-header-row">
        <p className="form-note">
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
        <Button variant="secondary" size="sm" onClick={handleSyncFirewall} disabled={firewallSyncing}>
          <Icon name="lock" size={14} />
          {firewallSyncing ? t("common.saving") : t("portsTab.syncFirewall")}
        </Button>
      </div>

      {loading ? (
        <SkeletonRows />
      ) : ports.length === 0 ? (
        <EmptyState icon="wifi" title={t("portsTab.emptyTitle")} description={t("portsTab.emptyDescription")} />
      ) : (
        <ul className="server-list">
          {ports.map((port) => (
            <li key={port.id} className="server-list-item">
              <div className="server-list-main">
                <span className="server-list-name" title={port.name}>
                  {port.name}
                </span>
                <span className="server-list-host">
                  {port.protocol.toUpperCase()} · {port.bindAddress}:{port.internalPort}
                  {port.externalPort ? ` → ${port.externalPort}` : ""}
                </span>
              </div>
              <Badge tone="neutral">{t(`applicationNetwork.visibility.${port.visibility}`)}</Badge>
              {port.required && <Badge tone="neutral">{t("portsTab.required")}</Badge>}
              <IconButton
                icon="edit"
                size="sm"
                title={t("portsTab.editAria", { name: port.name })}
                onClick={() => {
                  setEditingPort(port);
                  setFormOpen(true);
                }}
              />
              {!port.required && (
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
            </li>
          ))}
        </ul>
      )}

      {formOpen && (
        <PortFormModal
          applicationId={applicationId}
          application={application}
          editingPort={editingPort}
          onClose={() => setFormOpen(false)}
          onSaved={() => {
            setFormOpen(false);
            reload();
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
  application: ApplicationDetail;
  editingPort: ApplicationPort | null;
  onClose: () => void;
  onSaved: () => void;
}

function PortFormModal({ applicationId, application, editingPort, onClose, onSaved }: PortFormModalProps) {
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
      if (editingPort) {
        await updateApplicationPort(applicationId, editingPort.id, input);
      } else {
        await addApplicationPort(applicationId, input);
      }
      await recreateIfRunningDocker(application);
      onSaved();
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
                <select className="form-input" value={protocol} onChange={(e) => setProtocol(e.target.value as PortProtocol)}>
                  <option value="tcp">TCP</option>
                  <option value="udp">UDP</option>
                </select>
              </label>
              <label className="form-field form-field-grow">
                <span className="form-label">{t("applicationNetwork.access")}</span>
                <select className="form-input" value={visibility} onChange={(e) => setVisibility(e.target.value as PortVisibility)}>
                  <option value="public">{t("applicationNetwork.visibility.public")}</option>
                  <option value="vibeNetwork">{t("applicationNetwork.visibility.vibeNetwork")}</option>
                  <option value="localhost">{t("applicationNetwork.visibility.localhost")}</option>
                  <option value="custom">{t("applicationNetwork.visibility.custom")}</option>
                </select>
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
