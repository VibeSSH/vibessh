import { Trans, useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { useModalDialog } from "@/hooks/useModalDialog";
import "./AddServerModal.css";
import "./forms.css";

interface DeleteServerDialogProps {
  serverName: string;
  busy: boolean;
  error: string | null;
  onConfirm: () => void;
  onCancel: () => void;
}

export function DeleteServerDialog({ serverName, busy, error, onConfirm, onCancel }: DeleteServerDialogProps) {
  const { t } = useTranslation();
  const backdrop = useModalDialog(onCancel, { labelledBy: "deleteserverdialog-dialog-title-1" });
  return (
    <div className="modal-backdrop" {...backdrop.backdropProps}>
      <div className="modal-panel modal-panel-sm" {...backdrop.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="deleteserverdialog-dialog-title-1">{t("deleteDialog.title")}</h2>
          <IconButton icon="x" size="sm" onClick={onCancel} title={t("common.close")} />
        </div>
        <div className="modal-body">
          <p className="dialog-body-text">
            <Trans i18nKey="deleteDialog.body" values={{ name: serverName }} components={{ 1: <strong /> }} />
          </p>
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
