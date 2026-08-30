import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { useBackdropClose } from "@/hooks/useBackdropClose";
import { listApplications } from "@/services/applicationService";
import { createDatabaseHost, deleteDatabaseHost, listDatabaseHosts, setDatabaseHostPhpmyadmin } from "@/services/databaseService";
import { useServersStore } from "@/stores/serversStore";
import { toastSuccess } from "@/stores/toastStore";
import type { Application } from "@/types/application";
import type { CreateDatabaseHostInput, DatabaseEngine, DatabaseHost } from "@/types/database";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";
import "./pages.css";

/**
 * Global admin list of MySQL/MariaDB engines VibeSSH can provision
 * databases on (docs/APPLICATIONS_ARCHITECTURE.md Section 12) - a separate
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

  const [deletingHost, setDeletingHost] = useState<DatabaseHost | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const deleteBackdrop = useBackdropClose(() => !deleteBusy && setDeletingHost(null));

  const [linkingHost, setLinkingHost] = useState<DatabaseHost | null>(null);

  const reload = useCallback(() => {
    setLoading(true);
    setError(null);
    listDatabaseHosts()
      .then(setHosts)
      .catch((err) => setError(err instanceof Error ? err.message : t("databaseHosts.loadError")))
      .finally(() => setLoading(false));
  }, [t]);

  useEffect(reload, [reload]);

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
      setDeleteError(err instanceof Error ? err.message : t("databaseHosts.deleteError"));
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
                    <Icon name="database" size={16} />
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
                  <IconButton
                    icon="edit"
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
        <div className="modal-backdrop" {...deleteBackdrop}>
          <div className="modal-panel modal-panel-sm" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2 className="modal-title">{t("databaseHosts.deleteTitle")}</h2>
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
  onClose: () => void;
  onSaved: () => void;
}

function DatabaseHostFormModal({ onClose, onSaved }: DatabaseHostFormModalProps) {
  const { t } = useTranslation();
  const backdrop = useBackdropClose(onClose);
  const servers = useServersStore((s) => s.servers);

  const [name, setName] = useState("");
  const [serverId, setServerId] = useState("");
  const [engine, setEngine] = useState<DatabaseEngine>("mysql");
  const [host, setHost] = useState("127.0.0.1");
  const [port, setPort] = useState("3306");
  const [adminUsername, setAdminUsername] = useState("root");
  const [adminPassword, setAdminPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!name.trim() || !host.trim() || !adminUsername.trim() || !adminPassword) {
      setError(t("databaseHosts.invalidForm"));
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
      setError(err instanceof Error ? err.message : t("databaseHosts.saveError"));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" {...backdrop}>
      <div className="modal-panel" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2 className="modal-title">{t("databaseHosts.addTitle")}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <form className="server-form" onSubmit={handleSubmit}>
          <div className="modal-body">
            {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
            <label className="form-field">
              <span className="form-label">{t("databaseHosts.name")}</span>
              <input className="form-input" value={name} onChange={(e) => setName(e.target.value)} placeholder={t("databaseHosts.namePlaceholder")} />
            </label>
            <label className="form-field">
              <span className="form-label">{t("databaseHosts.linkedServer")}</span>
              <select className="form-input" value={serverId} onChange={(e) => setServerId(e.target.value)}>
                <option value="">{t("databaseHosts.noLinkedServer")}</option>
                {servers.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name}
                  </option>
                ))}
              </select>
            </label>
            <p className="form-note">{t("databaseHosts.linkedServerNote")}</p>
            <div className="form-row">
              <label className="form-field form-field-narrow">
                <span className="form-label">{t("databaseHosts.engine")}</span>
                <select className="form-input" value={engine} onChange={(e) => setEngine(e.target.value as DatabaseEngine)}>
                  <option value="mysql">MySQL</option>
                  <option value="mariadb">MariaDB</option>
                </select>
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
            <div className="form-row">
              <label className="form-field">
                <span className="form-label">{t("databaseHosts.adminUsername")}</span>
                <input className="form-input" value={adminUsername} onChange={(e) => setAdminUsername(e.target.value)} placeholder="root" />
              </label>
              <label className="form-field">
                <span className="form-label">{t("databaseHosts.adminPassword")}</span>
                <input className="form-input" type="password" value={adminPassword} onChange={(e) => setAdminPassword(e.target.value)} />
              </label>
            </div>
            <p className="form-note">{t("databaseHosts.adminPasswordNote")}</p>
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

/** No dedicated phpMyAdmin Blueprint needed - any existing Docker application (typically `generic-docker` with image `phpmyadmin/phpmyadmin`) can be linked here (docs/APPLICATIONS_ARCHITECTURE.md Section 12.3, Option A). */
function PhpmyadminLinkModal({ host, onClose, onSaved }: PhpmyadminLinkModalProps) {
  const { t } = useTranslation();
  const backdrop = useBackdropClose(onClose);
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
      setError(err instanceof Error ? err.message : t("databaseHosts.saveError"));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" {...backdrop}>
      <div className="modal-panel modal-panel-sm" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2 className="modal-title">{t("databaseHosts.configurePhpmyadminTitle")}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <form onSubmit={handleSubmit}>
          <div className="modal-body">
            <p className="form-note">{t("databaseHosts.phpmyadminHelp")}</p>
            {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
            {loading ? (
              <SkeletonRows />
            ) : applications.length === 0 ? (
              <p className="form-note">{t("databaseHosts.noDockerApplications")}</p>
            ) : (
              <label className="form-field">
                <span className="form-label">{t("databaseHosts.phpmyadminApplication")}</span>
                <select className="form-input" value={applicationId} onChange={(e) => setApplicationId(e.target.value)}>
                  <option value="">{t("databaseHosts.phpmyadminNone")}</option>
                  {applications.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.name}
                    </option>
                  ))}
                </select>
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
