import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { Trans, useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-shell";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { useModalDialog } from "@/hooks/useModalDialog";
import {
  createApplicationDatabase,
  deleteApplicationDatabase,
  getPhpmyadminUrl,
  listApplicationDatabases,
  listDatabaseHosts,
  resetApplicationDatabasePassword,
  revealApplicationDatabasePassword,
} from "@/services/databaseService";
import type { ApplicationDatabase, DatabaseHost } from "@/types/database";
import "@/components/servers/forms.css";
import "./DatabasesTab.css";
import { errorMessage } from "@/services/tauri";

interface DatabasesTabProps {
  applicationId: string;
}

/** docs/APPLICATIONS_ARCHITECTURE.md Section 12.2's Databases tab - list of provisioned databases, a "New Database" inline form (host picker + optional purpose text, everything else generated server-side), password reveal-on-click, per-row regenerate/remove, and "Open in phpMyAdmin" (only shown once a host has one linked - see DatabaseHosts.tsx) which opens the deployed instance in the system browser with the database name pre-filled. Login itself still happens in phpMyAdmin's own form - real SSO would need phpMyAdmin's `signon` auth mode configured against something, out of scope for this first pass (Section 12.2). */
export function DatabasesTab({ applicationId }: DatabasesTabProps) {
  const { t } = useTranslation();
  const [databases, setDatabases] = useState<ApplicationDatabase[]>([]);
  const [hosts, setHosts] = useState<DatabaseHost[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [databaseHostId, setDatabaseHostId] = useState("");
  const [purpose, setPurpose] = useState("");
  const [creating, setCreating] = useState(false);

  const [busyRowId, setBusyRowId] = useState<string | null>(null);
  const [rowError, setRowError] = useState<string | null>(null);

  const [revealingDatabase, setRevealingDatabase] = useState<ApplicationDatabase | null>(null);
  const [revealedPassword, setRevealedPassword] = useState<string | null>(null);
  const [revealBusy, setRevealBusy] = useState(false);
  const [revealError, setRevealError] = useState<string | null>(null);
  const revealBackdrop = useModalDialog(() => setRevealingDatabase(null), { labelledBy: "databasestab-dialog-title-1" });

  const [deletingDatabase, setDeletingDatabase] = useState<ApplicationDatabase | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const deleteBackdrop = useModalDialog(() => !deleteBusy && setDeletingDatabase(null), { labelledBy: "databasestab-dialog-title-2" });

  const reload = useCallback(() => {
    setLoading(true);
    setError(null);
    listApplicationDatabases(applicationId)
      .then(setDatabases)
      .catch((err) => setError(errorMessage(err, t)))
      .finally(() => setLoading(false));
  }, [applicationId, t]);

  useEffect(reload, [reload]);

  useEffect(() => {
    listDatabaseHosts()
      .then((loaded) => {
        setHosts(loaded);
        setDatabaseHostId((current) => current || loaded[0]?.id || "");
      })
      .catch(() => setHosts([]));
  }, []);

  function hostFor(databaseHostId: string): DatabaseHost | undefined {
    return hosts.find((h) => h.id === databaseHostId);
  }

  async function handleCreate(e: React.FormEvent) {
    e.preventDefault();
    if (!databaseHostId) return;
    setCreating(true);
    setError(null);
    try {
      await createApplicationDatabase(applicationId, databaseHostId, purpose.trim() || undefined);
      setPurpose("");
      reload();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setCreating(false);
    }
  }

  async function handleReveal(database: ApplicationDatabase) {
    setRevealingDatabase(database);
    setRevealedPassword(null);
    setRevealError(null);
    setRevealBusy(true);
    try {
      setRevealedPassword(await revealApplicationDatabasePassword(database.id));
    } catch (err) {
      setRevealError(errorMessage(err, t));
    } finally {
      setRevealBusy(false);
    }
  }

  async function handleResetPassword(database: ApplicationDatabase) {
    setBusyRowId(database.id);
    setRowError(null);
    try {
      const password = await resetApplicationDatabasePassword(database.id);
      setRevealingDatabase(database);
      setRevealedPassword(password);
      setRevealError(null);
    } catch (err) {
      setRowError(errorMessage(err, t));
    } finally {
      setBusyRowId(null);
    }
  }

  async function handleOpenPhpmyadmin(database: ApplicationDatabase) {
    setBusyRowId(database.id);
    setRowError(null);
    try {
      const url = await getPhpmyadminUrl(database.databaseHostId, database.databaseName);
      await open(url);
    } catch (err) {
      setRowError(errorMessage(err, t));
    } finally {
      setBusyRowId(null);
    }
  }

  async function handleConfirmDelete() {
    if (!deletingDatabase) return;
    setDeleteBusy(true);
    setDeleteError(null);
    try {
      await deleteApplicationDatabase(deletingDatabase.id);
      setDeletingDatabase(null);
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

      {loading ? (
        <SkeletonRows />
      ) : databases.length === 0 ? (
        <EmptyState icon="database" title={t("databasesTab.emptyTitle")} description={t("databasesTab.emptyDescription")} />
      ) : (
        <ul className="server-list">
          {databases.map((database) => {
            const host = hostFor(database.databaseHostId);
            const busy = busyRowId === database.id;
            return (
              <li key={database.id} className="server-list-item">
                <div className="server-list-icon">
                  <Icon name="database" size={16} />
                </div>
                <div className="server-list-main">
                  <span className="server-list-name" title={database.databaseName}>
                    {database.databaseName}
                  </span>
                  <span className="server-list-host">
                    {database.username}@{host ? `${host.host}:${host.port}` : t("databasesTab.unknownHost")}
                  </span>
                </div>
                <Badge tone="neutral">{database.connectionsFrom}</Badge>
                {host?.phpmyadminApplicationId && (
                  <Button variant="secondary" size="sm" onClick={() => handleOpenPhpmyadmin(database)} disabled={busy}>
                    <Icon name="database" size={14} />
                    {t("databasesTab.openPhpmyadmin")}
                  </Button>
                )}
                <IconButton
                  icon="eye"
                  size="sm"
                  title={t("databasesTab.revealPasswordAria", { name: database.databaseName })}
                  onClick={() => handleReveal(database)}
                  disabled={busy}
                />
                <IconButton
                  icon="refresh-cw"
                  size="sm"
                  title={t("databasesTab.resetPasswordAria", { name: database.databaseName })}
                  onClick={() => handleResetPassword(database)}
                  disabled={busy}
                />
                <IconButton
                  icon="trash"
                  size="sm"
                  danger
                  title={t("databasesTab.deleteAria", { name: database.databaseName })}
                  onClick={() => {
                    setDeleteError(null);
                    setDeletingDatabase(database);
                  }}
                  disabled={busy}
                />
              </li>
            );
          })}
        </ul>
      )}

      {rowError && <p className="form-note form-note-danger form-note-spaced">{rowError}</p>}

      {hosts.length === 0 ? (
        <p className="form-note">
          <Trans t={t} i18nKey="databasesTab.noHosts" components={{ 1: <Link to="/database-hosts" /> }} />
        </p>
      ) : (
        <form className="databases-tab-create-form" onSubmit={handleCreate}>
          <div className="form-row">
            <select className="form-input" value={databaseHostId} onChange={(e) => setDatabaseHostId(e.target.value)}>
              {hosts.map((h) => (
                <option key={h.id} value={h.id}>
                  {h.name}
                </option>
              ))}
            </select>
            <input
              className="form-input"
              placeholder={t("databasesTab.purposePlaceholder")}
              value={purpose}
              onChange={(e) => setPurpose(e.target.value)}
            />
            <Button type="submit" disabled={creating || !databaseHostId}>
              <Icon name="plus" size={14} />
              {creating ? t("common.loading") : t("databasesTab.newDatabase")}
            </Button>
          </div>
          <p className="form-note">{t("databasesTab.generatedNote")}</p>
        </form>
      )}

      {deletingDatabase && (
        <div className="modal-backdrop" {...deleteBackdrop.backdropProps}>
          <div className="modal-panel modal-panel-sm" {...deleteBackdrop.panelProps}>
            <div className="modal-header">
              <h2 className="modal-title" id="databasestab-dialog-title-1">{t("databasesTab.deleteTitle")}</h2>
              <IconButton icon="x" size="sm" onClick={() => setDeletingDatabase(null)} title={t("common.close")} />
            </div>
            <div className="modal-body">
              <p className="dialog-body-text">{t("databasesTab.deleteBody", { name: deletingDatabase.databaseName })}</p>
              {deleteError && <p className="form-note form-note-danger form-note-spaced">{deleteError}</p>}
              <div className="form-actions">
                <Button variant="secondary" onClick={() => setDeletingDatabase(null)} disabled={deleteBusy}>
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

      {revealingDatabase && (
        <div className="modal-backdrop" {...revealBackdrop.backdropProps}>
          <div className="modal-panel modal-panel-sm" {...revealBackdrop.panelProps}>
            <div className="modal-header">
              <h2 className="modal-title" id="databasestab-dialog-title-2">{t("databasesTab.credentialsTitle", { name: revealingDatabase.databaseName })}</h2>
              <IconButton icon="x" size="sm" onClick={() => setRevealingDatabase(null)} title={t("common.close")} />
            </div>
            <div className="modal-body">
              {revealError && <p className="form-note form-note-danger form-note-spaced">{revealError}</p>}
              {revealBusy ? (
                <SkeletonRows />
              ) : (
                <>
                  <CredentialRow label={t("databasesTab.credHost")} value={hostAddressFor(revealingDatabase, hosts, t)} />
                  <CredentialRow label={t("databasesTab.credDatabase")} value={revealingDatabase.databaseName} />
                  <CredentialRow label={t("databasesTab.credUser")} value={revealingDatabase.username} />
                  {revealedPassword !== null && <CredentialRow label={t("databasesTab.credPassword")} value={revealedPassword} />}
                </>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function hostAddressFor(database: ApplicationDatabase, hosts: DatabaseHost[], t: (key: string) => string): string {
  const host = hosts.find((h) => h.id === database.databaseHostId);
  return host ? `${host.host}:${host.port}` : t("databasesTab.unknownHost");
}

interface CredentialRowProps {
  label: string;
  value: string;
}

/** One copyable connection-detail field - reuses the same `.code-block`/`.code-block-copy` monospace-value-plus-copy-button pattern `InvitationsSection.tsx` already established for a single token, just repeated per field here (host/database/user/password). */
function CredentialRow({ label, value }: CredentialRowProps) {
  async function handleCopy() {
    try {
      await navigator.clipboard.writeText(value);
    } catch {
      // clipboard access denied - nothing useful to do about it here
    }
  }

  return (
    <label className="form-field">
      <span className="form-label">{label}</span>
      <div className="code-block">
        <span className="code-block-text">{value}</span>
        <button type="button" className="code-block-copy" onClick={handleCopy} aria-label={label}>
          <Icon name="copy" size={14} />
        </button>
      </div>
    </label>
  );
}
