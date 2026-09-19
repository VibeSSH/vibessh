import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Details } from "@/components/ui/Details";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { Select } from "@/components/ui/Select";
import { IconButton } from "@/components/ui/IconButton";
import { RowPicker, serverRowPickerOption } from "@/components/ui/RowPicker";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { useModalDialog } from "@/hooks/useModalDialog";
import { listApplications } from "@/services/applicationService";
import {
  createDatabaseHost,
  deleteDatabaseHost,
  listDatabaseHosts,
  repairDatabaseReachability,
  setDatabaseHostPhpmyadmin,
  updateDatabaseHost,
} from "@/services/databaseService";
import { useServersStore } from "@/stores/serversStore";
import { toastError, toastSuccess } from "@/stores/toastStore";
import type { Application } from "@/types/application";
import type { CreateDatabaseHostInput, DatabaseEngine, DatabaseHost } from "@/types/database";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";
import "./pages.css";
import { errorMessage } from "@/services/tauri";
import { NodeIcon } from "@/components/servers/NodeIcon";

/**
 * Global admin list of MySQL/MariaDB engines VibeSSH can provision
 * databases on (docs/architecture/APPLICATIONS_ARCHITECTURE.md Section 12) - a separate
 * top-level page, not a per-Server or per-Application tab, since one host
 * is meant to be reused across many applications, the same "registered
 * once, linked from many places" shape Servers already has.
 */
export function DatabaseHosts() {
  const { t } = useTranslation();
  const servers = useServersStore((s) => s.servers);

  const [hosts, setHosts] = useState<DatabaseHost[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [formOpen, setFormOpen] = useState(false);
  const [editingHost, setEditingHost] = useState<DatabaseHost | null>(null);

  const [deletingHost, setDeletingHost] = useState<DatabaseHost | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const deleteBackdrop = useModalDialog(() => !deleteBusy && setDeletingHost(null), { labelledBy: "databasehosts-dialog-title-1" });

  const [linkingHost, setLinkingHost] = useState<DatabaseHost | null>(null);
  /** The host whose reachability is being re-applied, so only its own button spins. */
  const [repairingHostId, setRepairingHostId] = useState<string | null>(null);

  const reload = useCallback(() => {
    setLoading(true);
    setError(null);
    listDatabaseHosts()
      .then(setHosts)
      .catch((err) => setError(errorMessage(err, t)))
      .finally(() => setLoading(false));
  }, [t]);

  useEffect(reload, [reload]);

  /**
   * Re-applies the two things a container needs to reach this database: the
   * server bound to the Docker bridge as well as loopback, and - only where
   * ufw is enforcing - a rule letting a container's packet in, scoped to the
   * bridge address so it opens nothing to the internet.
   *
   * Both are no-ops when they are already right, which is why this is safe
   * to press on a host that is working.
   */
  async function handleRepair(host: DatabaseHost) {
    setRepairingHostId(host.id);
    try {
      await repairDatabaseReachability(host.id);
      toastSuccess(t("databaseHosts.repairedToast", { name: host.name }));
    } catch (err) {
      toastError(errorMessage(err, t));
    } finally {
      setRepairingHostId(null);
    }
  }

  async function handleConfirmDelete() {
    if (!deletingHost) return;
    setDeleteBusy(true);
    setDeleteError(null);
    try {
      await deleteDatabaseHost(deletingHost.id);
      toastSuccess(t("databaseHosts.removedToast", { name: deletingHost.name }));
      setDeletingHost(null);
      reload();
    } catch (err) {
      setDeleteError(errorMessage(err, t));
    } finally {
      setDeleteBusy(false);
    }
  }

  return (
    <div className="page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">{t("databaseHosts.title")}</h1>
          <p className="page-subtitle">{t("databaseHosts.subtitle")}</p>
        </div>
        <Button onClick={() => setFormOpen(true)}>
          <Icon name="plus" size={16} />
          {t("databaseHosts.addHost")}
        </Button>
      </div>

      {error && <p className="page-error-note">{error}</p>}

      <Card>
        {loading ? (
          <SkeletonRows />
        ) : hosts.length === 0 ? (
          <EmptyState icon="database" title={t("databaseHosts.emptyTitle")} description={t("databaseHosts.emptyDescription")} />
        ) : (
          <ul className="server-list">
            {hosts.map((host) => {
              const serverName = host.serverId ? (servers.find((s) => s.id === host.serverId)?.name ?? host.serverId) : null;
              return (
                <li key={host.id} className="server-list-item">
                  <div className="server-list-icon">
                    {/* The node's icon when the host lives on one, so the row
                        says *where* at a glance; the database glyph when it
                        does not, which is the honest answer for an external
                        server VibeSSH only holds credentials for. */}
                    <NodeIcon server={servers.find((s) => s.id === host.serverId)} size={16} fallback="database" />
                  </div>
                  <div className="server-list-main">
                    <span className="server-list-name" title={host.name}>
                      {host.name}
                    </span>
                    <span className="server-list-host">
                      {host.adminUsername}@{host.host}:{host.port} · {serverName ?? t("databaseHosts.noLinkedServer")}
                    </span>
                  </div>
                  <Badge tone="neutral">{host.engine === "mariadb" ? "MariaDB" : "MySQL"}</Badge>
                  {host.phpmyadminApplicationId && <Badge tone="success">{t("databaseHosts.phpmyadminLinked")}</Badge>}
                  {/* Only for a server on a node VibeSSH manages - there is
                      nothing to configure on somebody else's host, and the
                      backend refuses it anyway. */}
                  {host.serverId && (
                    <IconButton
                      icon="refresh-cw"
                      size="sm"
                      disabled={repairingHostId === host.id}
                      title={t("databaseHosts.repairAria", { name: host.name })}
                      onClick={() => void handleRepair(host)}
                    />
                  )}
                  <IconButton icon="edit" size="sm" title={t("databaseHosts.editAria", { name: host.name })} onClick={() => setEditingHost(host)} />
                  {/* Its own icon now. Linking a web interface and fixing a
                      username are different jobs, and one pencil for both
                      meant the connection details could not be corrected at
                      all. */}
                  <IconButton
                    icon="external-link"
                    size="sm"
                    title={t("databaseHosts.configurePhpmyadminAria", { name: host.name })}
                    onClick={() => setLinkingHost(host)}
                  />
                  <IconButton
                    icon="trash"
                    size="sm"
                    danger
                    title={t("databaseHosts.deleteAria", { name: host.name })}
                    onClick={() => {
                      setDeleteError(null);
                      setDeletingHost(host);
                    }}
                  />
                </li>
              );
            })}
          </ul>
        )}
      </Card>

      {formOpen && (
        <DatabaseHostFormModal
          onClose={() => setFormOpen(false)}
          onSaved={() => {
            setFormOpen(false);
            reload();
          }}
        />
      )}

      {editingHost && (
        <DatabaseHostFormModal
          existing={editingHost}
          onClose={() => setEditingHost(null)}
          onSaved={() => {
            setEditingHost(null);
            reload();
          }}
        />
      )}

      {linkingHost && (
        <PhpmyadminLinkModal
          host={linkingHost}
          onClose={() => setLinkingHost(null)}
          onSaved={() => {
            setLinkingHost(null);
            reload();
          }}
        />
      )}

      {deletingHost && (
        <div className="modal-backdrop" {...deleteBackdrop.backdropProps}>
          <div className="modal-panel modal-panel-sm" {...deleteBackdrop.panelProps}>
            <div className="modal-header">
              <h2 className="modal-title" id="databasehosts-dialog-title-1">{t("databaseHosts.deleteTitle")}</h2>
              <IconButton icon="x" size="sm" onClick={() => setDeletingHost(null)} title={t("common.close")} />
            </div>
            <div className="modal-body">
              <p className="dialog-body-text">{t("databaseHosts.deleteBody", { name: deletingHost.name })}</p>
              {deleteError && <p className="form-note form-note-danger form-note-spaced">{deleteError}</p>}
              <div className="form-actions">
                <Button variant="secondary" onClick={() => setDeletingHost(null)} disabled={deleteBusy}>
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

interface DatabaseHostFormModalProps {
  /** The host being corrected. Absent means this is a new one. */
  existing?: DatabaseHost;
  onClose: () => void;
  onSaved: () => void;
}

function DatabaseHostFormModal({ existing, onClose, onSaved }: DatabaseHostFormModalProps) {
  const { t } = useTranslation();
  const backdrop = useModalDialog(onClose, { labelledBy: "databasehosts-dialog-title-2" });
  const servers = useServersStore((s) => s.servers);

  const [name, setName] = useState(existing?.name ?? "");
  const [serverId, setServerId] = useState(existing?.serverId ?? "");
  const [engine, setEngine] = useState<DatabaseEngine>(existing?.engine ?? "mysql");
  const [host, setHost] = useState(existing?.host ?? "127.0.0.1");
  const [port, setPort] = useState(String(existing?.port ?? 3306));
  const [adminUsername, setAdminUsername] = useState(existing?.adminUsername ?? "root");
  const [adminPassword, setAdminPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    // A password is required to create a host and optional to edit one: the
    // stored secret is kept when the field is left blank, so a port can be
    // fixed without retyping something the frontend has never held.
    if (!name.trim() || !host.trim() || !adminUsername.trim() || (!existing && !adminPassword)) {
      setError(t("databaseHosts.invalidForm"));
      return;
    }

    if (existing) {
      setBusy(true);
      setError(null);
      try {
        await updateDatabaseHost(existing.id, {
          name: name.trim(),
          host: host.trim(),
          port: Number(port) || 3306,
          adminUsername: adminUsername.trim(),
          adminPassword,
        });
        toastSuccess(t("databaseHosts.savedToast", { name: name.trim() }));
        onSaved();
      } catch (err) {
        setError(errorMessage(err, t));
      } finally {
        setBusy(false);
      }
      return;
    }
    const input: CreateDatabaseHostInput = {
      serverId: serverId || undefined,
      name: name.trim(),
      engine,
      host: host.trim(),
      port: Number(port) || 3306,
      adminUsername: adminUsername.trim(),
      adminPassword,
    };

    setBusy(true);
    setError(null);
    try {
      await createDatabaseHost(input);
      toastSuccess(t("databaseHosts.addedToast", { name: input.name }));
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
          <h2 className="modal-title" id="databasehosts-dialog-title-2">{existing ? t("databaseHosts.editTitle") : t("databaseHosts.addTitle")}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <form className="server-form" onSubmit={handleSubmit}>
          <div className="modal-body">
            {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
            <label className="form-field">
              <span className="form-label">{t("databaseHosts.name")}</span>
              <input className="form-input" value={name} onChange={(e) => setName(e.target.value)} placeholder={t("databaseHosts.namePlaceholder")} />
            </label>
            <RowPicker
              label={t("databaseHosts.linkedServer")}
              placeholder={t("databaseHosts.noLinkedServer")}
              value={serverId}
              onChange={setServerId}
              options={[{ id: "", icon: "x", name: t("databaseHosts.noLinkedServer") }, ...servers.map((s) => serverRowPickerOption(s, t))]}
            />
            <Details>
              <p className="form-note">{t("databaseHosts.linkedServerNote")}</p>
            </Details>
            <div className="form-row">
              <label className="form-field form-field-narrow">
                <span className="form-label">{t("databaseHosts.engine")}</span>
                <Select
                  value={engine}
                  onChange={(value) => setEngine(value as DatabaseEngine)}
                  items={[
                    { value: "mysql", label: "MySQL" },
                    { value: "mariadb", label: "MariaDB" },
                  ]}
                />
              </label>
              <label className="form-field form-field-grow">
                <span className="form-label">{t("databaseHosts.host")}</span>
                <input className="form-input" value={host} onChange={(e) => setHost(e.target.value)} placeholder="127.0.0.1" />
              </label>
              <label className="form-field form-field-narrow">
                <span className="form-label">{t("databaseHosts.port")}</span>
                <input className="form-input" type="number" min={1} max={65535} value={port} onChange={(e) => setPort(e.target.value)} placeholder="3306" />
              </label>
            </div>
            {/* Somebody read "Port" as the port their database runs on, changed
                it to 3307, and spent an evening on a plugin that could no
                longer open a socket. `update_database_host` isn't even async -
                it writes this row and touches nothing on the Node. */}
            <Details>
              <p className="form-note">{t("databaseHosts.addressNote")}</p>
            </Details>
            <div className="form-row">
              <label className="form-field">
                <span className="form-label">{t("databaseHosts.adminUsername")}</span>
                <input className="form-input" value={adminUsername} onChange={(e) => setAdminUsername(e.target.value)} placeholder="root" />
              </label>
              <label className="form-field">
                <span className="form-label">{t("databaseHosts.adminPassword")}</span>
                <input
                  className="form-input"
                  type="password"
                  value={adminPassword}
                  onChange={(e) => setAdminPassword(e.target.value)}
                  placeholder={existing ? t("databaseHosts.adminPasswordKeep") : undefined}
                />
              </label>
            </div>
            <Details>
              <p className="form-note">{t("databaseHosts.adminPasswordNote")}</p>
            </Details>
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

interface PhpmyadminLinkModalProps {
  host: DatabaseHost;
  onClose: () => void;
  onSaved: () => void;
}

/** No dedicated phpMyAdmin Blueprint needed - any existing Docker application (typically `generic-docker` with image `phpmyadmin/phpmyadmin`) can be linked here (docs/architecture/APPLICATIONS_ARCHITECTURE.md Section 12.3, Option A). */
function PhpmyadminLinkModal({ host, onClose, onSaved }: PhpmyadminLinkModalProps) {
  const { t } = useTranslation();
  const backdrop = useModalDialog(onClose, { labelledBy: "databasehosts-dialog-title-3" });
  const [applications, setApplications] = useState<Application[]>([]);
  const [loading, setLoading] = useState(true);
  const [applicationId, setApplicationId] = useState(host.phpmyadminApplicationId ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    listApplications()
      .then((all) => setApplications(all.filter((a) => a.runtimeType === "docker")))
      .catch(() => setApplications([]))
      .finally(() => setLoading(false));
  }, []);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await setDatabaseHostPhpmyadmin(host.id, applicationId || undefined);
      onSaved();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" {...backdrop.backdropProps}>
      <div className="modal-panel modal-panel-sm" {...backdrop.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="databasehosts-dialog-title-3">{t("databaseHosts.configurePhpmyadminTitle")}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <form onSubmit={handleSubmit}>
          <div className="modal-body">
            <Details>
              <p className="form-note">{t("databaseHosts.phpmyadminHelp")}</p>
            </Details>
            {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
            {loading ? (
              <SkeletonRows />
            ) : applications.length === 0 ? (
              <p className="form-note">{t("databaseHosts.noDockerApplications")}</p>
            ) : (
              <label className="form-field">
                <span className="form-label">{t("databaseHosts.phpmyadminApplication")}</span>
                <Select
                  value={applicationId}
                  onChange={setApplicationId}
                  items={[
                    { value: "", label: t("databaseHosts.phpmyadminNone") },
                    ...applications.map((a) => ({ value: a.id, label: a.name })),
                  ]}
                />
              </label>
            )}
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
