import { Trans, useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Checkbox } from "@/components/ui/Checkbox";
import { Dialog } from "@/components/ui/Dialog";
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
  return (
    // Not dismissable while the deletion is in flight: a stray Escape used to
    // take the dialog away mid-delete, leaving the outcome of a destructive
    // action to be discovered from the list rather than reported here.
    <Dialog
      open
      onClose={onCancel}
      size="sm"
      dismissable={!busy}
      title={count && count > 1 ? t("deleteApplicationDialog.titleMany", { count }) : t("deleteApplicationDialog.title")}
    >
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
        <div className="dialog-option">
          <Checkbox checked={removeFiles} onChange={onRemoveFilesChange} disabled={busy} label={t("deleteApplicationDialog.removeFiles")} />
          <p className="form-note dialog-option-note">{t("deleteApplicationDialog.removeFilesNote")}</p>
        </div>
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
    </Dialog>
  );
}
