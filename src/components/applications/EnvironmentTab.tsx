import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Checkbox } from "@/components/ui/Checkbox";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { useModalDialog } from "@/hooks/useModalDialog";
import { recreateApplication, refreshApplicationStatus, setApplicationEnvironment } from "@/services/applicationService";
import type { ApplicationDetail, Blueprint, EnvironmentVariable } from "@/types/application";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";
import { errorMessage } from "@/services/tauri";

/** A Docker container's environment is baked in at `docker create` time
 * (see `runtime::docker`'s own doc comment) - a plain restart reuses the
 * same, now-stale container, so an edited/added/removed variable would
 * silently never take effect. Same Pterodactyl-matching "change it, it just
 * works" auto-recreate `ApplicationConfigCard`/`ResourceLimitsCard`/`PortsTab`
 * already do for their own saves - a stopped app is left stopped.
 *
 * Checks the *real*, freshly-probed status (`refreshApplicationStatus`)
 * rather than trusting `application.status` as passed down - that prop is
 * only ever updated by an explicit start/stop/restart/recreate/kill action
 * or the next 5s poll tick (see `models::ApplicationStatus`'s own "never
 * trusted as sole truth" doc comment), so it can still say "stopped" for a
 * few seconds right after the app was actually started - long enough to
 * silently skip the recreate this save depends on. */
async function recreateIfRunningDocker(application: ApplicationDetail) {
  if (application.runtimeType !== "docker") return;
  const status = await refreshApplicationStatus(application.id);
  if (status === "running") {
    await recreateApplication(application.id);
  }
}

interface EnvironmentTabProps {
  application: ApplicationDetail;
  /** Only for the note below - `null` while it loads, and absent for a
   *  blueprint that points at nothing, which is most of them. */
  blueprint?: Blueprint | null;
  onSaved: () => void;
}

/**
 * Explains the two variables nobody typed.
 *
 * A blueprint that `connectsTo` another application has its host and port
 * written here once, when the application is created, from whichever target
 * was picked in the wizard. Nothing updates them afterwards, and the port is
 * the one the target listens on *inside its own container* - so somebody who
 * publishes their MariaDB on 3307 and comes here expecting `PMA_PORT` to have
 * followed is looking at a value that is both unchanged and correct. That was
 * reported as a bug, which is fair: the interface said nothing either way.
 *
 * Driven by the blueprint's own declaration rather than by naming phpMyAdmin,
 * so the next blueprint that points at a sibling service explains itself for
 * free.
 */
export function connectionNote(application: ApplicationDetail, blueprint: Blueprint | null | undefined): { host: string; port: string } | null {
  const connection = blueprint?.connectsTo;
  if (!connection) return null;
  const present = (key: string) => application.environment.some((row) => row.key === key);
  if (!present(connection.hostEnv) && !present(connection.portEnv)) return null;
  return { host: connection.hostEnv, port: connection.portEnv };
}

export function EnvironmentTab({ application, blueprint, onSaved }: EnvironmentTabProps) {
  const { t } = useTranslation();
  const [formOpen, setFormOpen] = useState(false);
  const [editingKey, setEditingKey] = useState<string | null>(null);
  const [deletingKey, setDeletingKey] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const connection = connectionNote(application, blueprint);

  async function persist(next: EnvironmentVariable[]) {
    setBusy(true);
    setError(null);
    try {
      await setApplicationEnvironment(application.id, next);
      await recreateIfRunningDocker(application);
      onSaved();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  async function handleConfirmDelete() {
    if (!deletingKey) return;
    const next = application.environment.filter((row) => row.key !== deletingKey);
    setDeletingKey(null);
    await persist(next);
  }

  const editing = editingKey ? (application.environment.find((row) => row.key === editingKey) ?? null) : null;

  return (
    <div className="application-detail-overview">
      <div className="application-detail-header-row">
        <p className="form-note">
          {application.runtimeType === "docker"
            ? application.status === "running"
              ? t("applicationConfig.recreateAutoNote")
              : t("applicationConfig.recreateStoppedNote")
            : t("applicationConfig.restartNote")}
        </p>
        <Button
          size="sm"
          onClick={() => {
            setEditingKey(null);
            setFormOpen(true);
          }}
          disabled={busy}
        >
          <Icon name="plus" size={14} />
          {t("applicationDetail.addEnvVar")}
        </Button>
      </div>

      {error && <p className="page-error-note">{error}</p>}

      {connection && <p className="form-note">{t("applicationDetail.connectionEnvNote", { host: connection.host, port: connection.port })}</p>}

      <Card>
        {busy ? (
          <SkeletonRows />
        ) : application.environment.length === 0 ? (
          <EmptyState icon="settings" title={t("applicationDetail.environmentEmptyTitle")} description={t("applicationDetail.environmentEmpty")} />
        ) : (
          <ul className="server-list">
            {application.environment.map((row) => (
              <li key={row.key} className="server-list-item">
                <div className="server-list-main">
                  <span className="server-list-name" title={row.key}>
                    {row.key}
                    {row.isSecret && (
                      <span title={t("applicationDetail.envSecretBadge")} style={{ marginLeft: 6, verticalAlign: "middle", display: "inline-flex" }}>
                        <Icon name="lock" size={12} />
                      </span>
                    )}
                  </span>
                  <span className="server-list-host">{row.isSecret ? t("applicationDetail.envSecretMasked") : row.value}</span>
                </div>
                <IconButton
                  icon="edit"
                  size="sm"
                  title={t("portsTab.editAria", { name: row.key })}
                  onClick={() => {
                    setEditingKey(row.key);
                    setFormOpen(true);
                  }}
                  disabled={busy}
                />
                <IconButton
                  icon="trash"
                  size="sm"
                  danger
                  title={t("portsTab.deleteAria", { name: row.key })}
                  onClick={() => setDeletingKey(row.key)}
                  disabled={busy}
                />
              </li>
            ))}
          </ul>
        )}
      </Card>

      {formOpen && (
        <EnvVarFormModal
          editing={editing}
          existingKeys={application.environment.map((row) => row.key)}
          onClose={() => setFormOpen(false)}
          onSubmit={async (key, value, isSecret) => {
            const withoutOld = application.environment.filter((row) => row.key !== editingKey);
            setFormOpen(false);
            await persist([...withoutOld, { key, value, isSecret }]);
          }}
        />
      )}

      {deletingKey && <DeleteEnvVarDialog envKey={deletingKey} onCancel={() => setDeletingKey(null)} onConfirm={handleConfirmDelete} />}
    </div>
  );
}

interface EnvVarFormModalProps {
  editing: EnvironmentVariable | null;
  existingKeys: string[];
  onClose: () => void;
  onSubmit: (key: string, value: string, isSecret: boolean) => Promise<void>;
}

function EnvVarFormModal({ editing, existingKeys, onClose, onSubmit }: EnvVarFormModalProps) {
  const { t } = useTranslation();
  const backdrop = useModalDialog(onClose, { labelledBy: "environmenttab-dialog-title-1" });
  const isEditing = Boolean(editing);
  const [key, setKey] = useState(editing?.key ?? "");
  const [value, setValue] = useState(editing?.value ?? "");
  const [isSecret, setIsSecret] = useState(editing?.isSecret ?? false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // A secret row's real value never reaches this form on edit (the backend
  // never sends it back - see the `EnvironmentVariable` type's own doc
  // comment), so a blank value only means "keep it" when the row was
  // already secret before *and* is staying secret now - any other case
  // (a new variable, or un-checking "secret" without retyping the real
  // value to keep as plaintext) has nothing to fall back to.
  const blankValueKeepsCurrent = isEditing && Boolean(editing?.isSecret) && isSecret;

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const trimmedKey = key.trim();
    if (!trimmedKey) {
      setError(t("applicationDetail.envInvalidForm"));
      return;
    }
    if (trimmedKey !== editing?.key && existingKeys.includes(trimmedKey)) {
      setError(t("applicationDetail.envDuplicateKey"));
      return;
    }
    if (!value && !blankValueKeepsCurrent) {
      setError(t("applicationDetail.envValueRequired"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await onSubmit(trimmedKey, value, isSecret);
    } catch (err) {
      setError(errorMessage(err, t));
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" {...backdrop.backdropProps}>
      <div className="modal-panel modal-panel-sm" {...backdrop.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="environmenttab-dialog-title-1">{isEditing ? t("applicationDetail.editEnvVar") : t("applicationDetail.addEnvVar")}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <form className="server-form" onSubmit={handleSubmit}>
          <div className="modal-body">
            {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
            <label className="form-field">
              <span className="form-label">{t("applicationDetail.envKey")}</span>
              <input className="form-input" value={key} onChange={(e) => setKey(e.target.value)} placeholder="PMA_HOST" autoFocus />
            </label>
            <label className="form-field">
              <span className="form-label">{t("applicationDetail.envValue")}</span>
              <input
                className="form-input"
                type={isSecret ? "password" : "text"}
                value={value}
                onChange={(e) => setValue(e.target.value)}
                placeholder={blankValueKeepsCurrent ? t("applicationDetail.envSecretValuePlaceholder") : "host.docker.internal"}
                autoComplete="off"
              />
            </label>
            <Checkbox checked={isSecret} onChange={setIsSecret} label={t("applicationDetail.envSecretLabel")} />
            {isSecret && <p className="form-note">{t("applicationDetail.envSecretHint")}</p>}
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

/**
 * Its own component rather than JSX inside a conditional, because
 * `useModalDialog` has an effect: it must run when the dialog mounts, not
 * when the tab does. The previous inline form called a hook inside a
 * conditional branch - a rules-of-hooks violation that was merely harmless
 * with the ref-only hook it used before.
 */
function DeleteEnvVarDialog({ envKey, onCancel, onConfirm }: { envKey: string; onCancel: () => void; onConfirm: () => void }) {
  const { t } = useTranslation();
  const dialog = useModalDialog(onCancel, { labelledBy: "delete-env-var-title" });
  return (
    <div className="modal-backdrop" {...dialog.backdropProps}>
      <div className="modal-panel modal-panel-sm" {...dialog.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="delete-env-var-title">
            {t("applicationDetail.deleteEnvVarTitle")}
          </h2>
          <IconButton icon="x" size="sm" onClick={onCancel} title={t("common.close")} />
        </div>
        <div className="modal-body">
          <p className="dialog-body-text">{t("applicationDetail.deleteEnvVarBody", { name: envKey })}</p>
          <div className="form-actions">
            <Button variant="secondary" onClick={onCancel}>
              {t("common.cancel")}
            </Button>
            <Button variant="danger" onClick={onConfirm}>
              {t("common.remove")}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
