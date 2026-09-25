import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Checkbox } from "@/components/ui/Checkbox";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { useModalDialog } from "@/hooks/useModalDialog";
import { useContainerApply } from "@/hooks/useContainerApply";
import { setApplicationEnvironment } from "@/services/applicationService";
import type { ApplicationDetail, Blueprint, EnvironmentVariable } from "@/types/application";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";
import { errorMessage } from "@/services/tauri";
import { toastSuccess } from "@/stores/toastStore";

interface EnvironmentTabProps {
  application: ApplicationDetail;
  /** Only for the note below - `null` while it loads, and absent for a
   *  blueprint that points at nothing, which is most of them. */
  blueprint?: Blueprint | null;
  /** Writes the mutation's own returned application straight into the cache,
   *  so the list updates at once instead of waiting on a second SSH refetch. */
  onApplied: (updated: ApplicationDetail) => void;
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

/** A variable whose name says it holds a credential - masked until revealed. */
function looksSensitive(key: string): boolean {
  return /(PASS|SECRET|TOKEN|PRIVATE|CREDENTIAL|API_?KEY|_KEY$)/i.test(key);
}

export function EnvironmentTab({ application, blueprint, onApplied }: EnvironmentTabProps) {
  const { t } = useTranslation();
  const applyToContainer = useContainerApply();
  const [formOpen, setFormOpen] = useState(false);
  const [editingKey, setEditingKey] = useState<string | null>(null);
  // Sensitive-looking values shown on request, one row at a time, and only
  // for as long as this screen is open.
  const [revealed, setRevealed] = useState<Set<string>>(() => new Set());
  const toggleRevealed = (key: string) =>
    setRevealed((previous) => {
      const next = new Set(previous);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  const [deletingKey, setDeletingKey] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const connection = connectionNote(application, blueprint);

  async function persist(next: EnvironmentVariable[]) {
    setBusy(true);
    setError(null);
    try {
      // The write itself is a quick DB update that returns the fresh
      // application - show it at once, then let the container recreate (the
      // slow part) run in the background rather than freezing the list on it.
      const updated = await setApplicationEnvironment(application.id, next);
      onApplied(updated);
      void applyToContainer(updated);
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
      {error && <p className="page-error-note">{error}</p>}

      <Card
        title={t("applicationDetail.environmentTitle")}
        subtitle={
          application.runtimeType === "docker"
            ? application.status === "running"
              ? t("applicationConfig.recreateAutoNote")
              : t("applicationConfig.recreateStoppedNote")
            : t("applicationConfig.restartNote")
        }
        actions={
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
        }
      >
        {connection && <p className="form-note">{t("applicationDetail.connectionEnvNote", { host: connection.host, port: connection.port })}</p>}
        {application.environment.length === 0 ? (
          <EmptyState icon="settings" title={t("applicationDetail.environmentEmptyTitle")} description={t("applicationDetail.environmentEmpty")} />
        ) : (
          <ul className="server-list">
            {application.environment.map((row) => {
              // Masked unless asked: a stored secret always, and anything
              // whose name says it is one - a DB password added as a plain
              // variable used to sit on this screen in the clear, and so on
              // every screenshot of it.
              const masked = row.isSecret || (looksSensitive(row.key) && !revealed.has(row.key));
              return (
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
                  <span className="server-list-host env-value">{masked ? t("applicationDetail.envSecretMasked") : row.value}</span>
                </div>
                {!row.isSecret && looksSensitive(row.key) && (
                  <IconButton
                    icon={revealed.has(row.key) ? "eye-off" : "eye"}
                    size="sm"
                    title={revealed.has(row.key) ? t("applicationDetail.envHide", { name: row.key }) : t("applicationDetail.envReveal", { name: row.key })}
                    onClick={() => toggleRevealed(row.key)}
                  />
                )}
                {!row.isSecret && (
                  <IconButton
                    icon="copy"
                    size="sm"
                    title={t("applicationDetail.envCopy", { name: row.key })}
                    onClick={() => {
                      navigator.clipboard
                        .writeText(row.value)
                        .then(() => toastSuccess(t("applicationDetail.envCopied", { name: row.key })))
                        .catch(() => {});
                    }}
                  />
                )}
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
              );
            })}
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
