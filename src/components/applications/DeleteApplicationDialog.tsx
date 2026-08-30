import { Trans, useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";

interface DeleteApplicationDialogProps {
  applicationName: string;
  busy: boolean;
  error: string | null;
  onConfirm: () => void;
  onCancel: () => void;
}

export function DeleteApplicationDialog({ applicationName, busy, error, onConfirm, onCancel }: DeleteApplicationDialogProps) {
  const { t } = useTranslation();
  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal-panel modal-panel-sm" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2 className="modal-title">{t("deleteApplicationDialog.title")}</h2>
          <IconButton icon="x" size="sm" onClick={onCancel} title={t("common.close")} />
        </div>
        <div className="modal-body">
          <p className="dialog-body-text">
            <Trans i18nKey="deleteApplicationDialog.body" values={{ name: applicationName }} components={{ 1: <strong /> }} />
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
