import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { useModalDialog } from "@/hooks/useModalDialog";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";

interface JarReplaceWarningModalProps {
  fileName: string;
  isRunning: boolean;
  onCancel: () => void;
  onUploadOnly: () => void;
  onUploadAndRestart: () => void;
}

/** design brief section 123 - shown when an upload's destination name matches the jar the current startup command actually launches. Never restarts automatically without the user's own choice. */
export function JarReplaceWarningModal({ fileName, isRunning, onCancel, onUploadOnly, onUploadAndRestart }: JarReplaceWarningModalProps) {
  const { t } = useTranslation();
  const backdrop = useModalDialog(onCancel, { labelledBy: "jarreplacewarningmodal-dialog-title-1" });

  return (
    <div className="modal-backdrop" {...backdrop.backdropProps}>
      <div className="modal-panel modal-panel-sm" {...backdrop.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="jarreplacewarningmodal-dialog-title-1">{t("applicationFilesTab.jarReplaceTitle")}</h2>
          <IconButton icon="x" size="sm" onClick={onCancel} title={t("common.close")} />
        </div>
        <div className="modal-body">
          <p className="dialog-body-text">{t("applicationFilesTab.jarReplaceBody", { name: fileName })}</p>
          <div className="form-actions">
            <Button variant="secondary" onClick={onCancel}>
              {t("common.cancel")}
            </Button>
            <Button variant="secondary" onClick={onUploadOnly}>
              {t("applicationFilesTab.uploadOnly")}
            </Button>
            {isRunning && (
              <Button variant="danger" onClick={onUploadAndRestart}>
                {t("applicationFilesTab.uploadAndRestart")}
              </Button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
