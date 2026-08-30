import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { useBackdropClose } from "@/hooks/useBackdropClose";
import { addApplicationPort, listApplicationPorts, removeApplicationPort, updateApplicationPort } from "@/services/applicationService";
import type { ApplicationPort, PortInput, PortProtocol } from "@/types/application";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";

interface PortsTabProps {
  applicationId: string;
}

/** Declared ports are documentation of intent, not a live guarantee - see applicationService.listApplicationPorts's own doc comment. A "required" port (blueprint-declared, none of the built-in blueprints set one up yet) can be edited but not removed here, same rule the backend itself enforces. */
export function PortsTab({ applicationId }: PortsTabProps) {
  const { t } = useTranslation();
  const [ports, setPorts] = useState<ApplicationPort[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [formOpen, setFormOpen] = useState(false);
  const [editingPort, setEditingPort] = useState<ApplicationPort | null>(null);
  const [deletingPort, setDeletingPort] = useState<ApplicationPort | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const deleteBackdrop = useBackdropClose(() => !deleteBusy && setDeletingPort(null));

  const reload = useCallback(() => {
    setLoading(true);
    setError(null);
    listApplicationPorts(applicationId)
      .then(setPorts)
      .catch((err) => setError(err instanceof Error ? err.message : t("portsTab.loadError")))
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
      reload();
    } catch (err) {
      setDeleteError(err instanceof Error ? err.message : t("portsTab.deleteError"));
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
          editingPort={editingPort}
          onClose={() => setFormOpen(false)}
          onSaved={() => {
            setFormOpen(false);
            reload();
          }}
        />
      )}

      {deletingPort && (
        <div className="modal-backdrop" {...deleteBackdrop}>
          <div className="modal-panel modal-panel-sm" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2 className="modal-title">{t("portsTab.deleteTitle")}</h2>
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
  onSaved: () => void;
}

function PortFormModal({ applicationId, editingPort, onClose, onSaved }: PortFormModalProps) {
  const { t } = useTranslation();
  const backdrop = useBackdropClose(onClose);
  const isEditing = Boolean(editingPort);

  const [name, setName] = useState(editingPort?.name ?? "");
  const [protocol, setProtocol] = useState<PortProtocol>(editingPort?.protocol ?? "tcp");
  const [bindAddress, setBindAddress] = useState(editingPort?.bindAddress ?? "0.0.0.0");
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
      bindAddress: bindAddress.trim() || "0.0.0.0",
      internalPort: internal,
      externalPort: external,
    };

    setBusy(true);
    setError(null);
    try {
      if (editingPort) {
        await updateApplicationPort(applicationId, editingPort.id, input);
      } else {
        await addApplicationPort(applicationId, input);
      }
      onSaved();
    } catch (err) {
      setError(err instanceof Error ? err.message : t("portsTab.saveError"));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" {...backdrop}>
      <div className="modal-panel" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2 className="modal-title">{isEditing ? t("portsTab.editTitle") : t("portsTab.addTitle")}</h2>
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
                <span className="form-label">{t("portsTab.bindAddress")}</span>
                <input className="form-input" value={bindAddress} onChange={(e) => setBindAddress(e.target.value)} placeholder="0.0.0.0" />
              </label>
            </div>
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
