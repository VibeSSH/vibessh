import { Trans, useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { useModalDialog } from "@/hooks/useModalDialog";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";

interface DeleteApplicationDialogProps {
  /** One name, or several when a selection is being removed together. */
  applicationName: string;
  /** How many are going, when it is more than one. */
  count?: number;
  busy: boolean;
  error: string | null;
  removeFiles: boolean;
  onRemoveFilesChange: (removeFiles: boolean) => void;
  onConfirm: () => void;
  onCancel: () => void;
}

export function DeleteApplicationDialog({
  applicationName,
  count,
  busy,
  error,
  removeFiles,
  onRemoveFilesChange,
  onConfirm,
  onCancel,
}: DeleteApplicationDialogProps) {
  const { t } = useTranslation();
  const backdrop = useModalDialog(onCancel, { labelledBy: "deleteapplicationdialog-dialog-title-1" });
  return (
    <div className="modal-backdrop" {...backdrop.backdropProps}>
      <div className="modal-panel modal-panel-sm" {...backdrop.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="deleteapplicationdialog-dialog-title-1">
            {count && count > 1 ? t("deleteApplicationDialog.titleMany", { count }) : t("deleteApplicationDialog.title")}
          </h2>
          <IconButton icon="x" size="sm" onClick={onCancel} title={t("common.close")} />
        </div>
        <div className="modal-body">
          <p className="dialog-body-text">
            {count && count > 1 ? (
              <Trans i18nKey="deleteApplicationDialog.bodyMany" count={count} values={{ count, names: applicationName }} components={{ 1: <strong /> }} />
            ) : (
              <Trans i18nKey="deleteApplicationDialog.body" values={{ name: applicationName }} components={{ 1: <strong /> }} />
            )}
          </p>
          {/* Off by default, and the one step here that cannot be undone.
              Offered at all because a re-import copies into the same
              directory and `cp -a` merges rather than replaces - so keeping
              the old files leaves anything deleted at the source behind
              forever. */}
          <label className="form-checkbox">
            <input type="checkbox" checked={removeFiles} onChange={(event) => onRemoveFilesChange(event.target.checked)} disabled={busy} />
            <span>{t("deleteApplicationDialog.removeFiles")}</span>
          </label>
          <p className="form-note">{t("deleteApplicationDialog.removeFilesNote")}</p>
          {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
          <div className="form-actions">
            <Button variant="secondary" onClick={onCancel} disabled={busy}>
              {t("common.cancel")}
            </Button>
            <Button variant="danger" onClick={onConfirm} disabled={busy}>
              {t("common.remove")}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
