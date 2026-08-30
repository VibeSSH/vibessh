import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { Trans, useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { useBackdropClose } from "@/hooks/useBackdropClose";
import {
  createApplicationDatabase,
  deleteApplicationDatabase,
  listApplicationDatabases,
  listDatabaseHosts,
  resetApplicationDatabasePassword,
  revealApplicationDatabasePassword,
} from "@/services/databaseService";
import type { ApplicationDatabase, DatabaseHost } from "@/types/database";
import "@/components/servers/forms.css";
import "./DatabasesTab.css";

interface DatabasesTabProps {
  applicationId: string;
}

/** docs/APPLICATIONS_ARCHITECTURE.md Section 12.2's Databases tab - list of provisioned databases, a "New Database" inline form (host picker + optional purpose text, everything else generated server-side), password reveal-on-click, and per-row regenerate/remove. The "Open in phpMyAdmin" button from that same section is deliberately not built yet: it needs `tauri-plugin-shell` (not a dependency) and a real, browser-reachable URL for the deployed phpMyAdmin container, which needs Docker port publishing (`runtime::docker` doesn't wire `-p` yet, a separate known gap) - shipping a button that can't actually open anything would be exactly the "half-working feature" the architecture doc's own Section 39 says not to ship. */
export function DatabasesTab({ applicationId }: DatabasesTabProps) {
  const { t } = useTranslation();
  const [databases, setDatabases] = useState<ApplicationDatabase[]>([]);
  const [hosts, setHosts] = useState<DatabaseHost[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [databaseHostId, setDatabaseHostId] = useState("");
  const [purpose, setPurpose] = useState("");
  const [creating, setCreating] = useState(false);

  const [revealed, setRevealed] = useState<Record<string, string>>({});
  const [busyRowId, setBusyRowId] = useState<string | null>(null);
  const [rowError, setRowError] = useState<string | null>(null);

  const [deletingDatabase, setDeletingDatabase] = useState<ApplicationDatabase | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const deleteBackdrop = useBackdropClose(() => !deleteBusy && setDeletingDatabase(null));

  const reload = useCallback(() => {
    setLoading(true);
    setError(null);
    listApplicationDatabases(applicationId)
      .then(setDatabases)
      .catch((err) => setError(err instanceof Error ? err.message : t("databasesTab.loadError")))
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
      setError(err instanceof Error ? err.message : t("databasesTab.createError"));
    } finally {
      setCreating(false);
    }
  }

  async function toggleReveal(database: ApplicationDatabase) {
    if (revealed[database.id] !== undefined) {
      setRevealed((prev) => {
        const next = { ...prev };
        delete next[database.id];
        return next;
      });
      return;
    }
    setBusyRowId(database.id);
    setRowError(null);
    try {
      const password = await revealApplicationDatabasePassword(database.id);
      setRevealed((prev) => ({ ...prev, [database.id]: password }));
    } catch (err) {
      setRowError(err instanceof Error ? err.message : t("databasesTab.revealError"));
    } finally {
      setBusyRowId(null);
    }
  }

  async function handleResetPassword(database: ApplicationDatabase) {
    setBusyRowId(database.id);
    setRowError(null);
    try {
      const password = await resetApplicationDatabasePassword(database.id);
      setRevealed((prev) => ({ ...prev, [database.id]: password }));
    } catch (err) {
      setRowError(err instanceof Error ? err.message : t("databasesTab.resetError"));
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
      setDeleteError(err instanceof Error ? err.message : t("databasesTab.deleteError"));
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
            const password = revealed[database.id];
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
                  {password !== undefined && (
                    <span className="server-list-host databases-tab-password" title={t("databasesTab.password")}>
                      <Icon name="key" size={12} />
                      {password}
                    </span>
                  )}
                </div>
                <Badge tone="neutral">{database.connectionsFrom}</Badge>
                <IconButton
                  icon={password !== undefined ? "eye-off" : "eye"}
                  size="sm"
                  title={t(password !== undefined ? "databasesTab.hidePasswordAria" : "databasesTab.revealPasswordAria", { name: database.databaseName })}
                  onClick={() => toggleReveal(database)}
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
        <div className="modal-backdrop" {...deleteBackdrop}>
          <div className="modal-panel modal-panel-sm" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2 className="modal-title">{t("databasesTab.deleteTitle")}</h2>
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
    </div>
  );
}
