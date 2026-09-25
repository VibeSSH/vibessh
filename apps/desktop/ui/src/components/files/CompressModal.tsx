import { FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { useModalDialog } from "@/hooks/useModalDialog";
import { errorMessage } from "@/services/tauri";
import type { RemoteFileEntry } from "@/types/files";

export interface CompressModalProps {
  targets: RemoteFileEntry[];
  onClose: () => void;
  onConfirm: (archiveName: string) => Promise<void>;
}

/** Names the archive, then compresses `targets` into it inside the current directory - "spakuj" in the row/selection context menu. */
export function CompressModal({ targets, onClose, onConfirm }: CompressModalProps) {
  const { t } = useTranslation();
  const backdrop = useModalDialog(onClose, { labelledBy: "files-dialog-title-2" });
  const defaultName = targets.length === 1 ? targets[0].name.replace(/\.[^./]+$/, "") : "archive";
  const [name, setName] = useState(defaultName);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    const trimmed = name.trim();
    if (!trimmed) return;
    setBusy(true);
    setError(null);
    try {
      await onConfirm(trimmed.toLowerCase().endsWith(".zip") ? trimmed : `${trimmed}.zip`);
      onClose();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" {...backdrop.backdropProps}>
      <div className="modal-panel modal-panel-sm" {...backdrop.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="files-dialog-title-2">{t("filesPage.compressTitle")}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <form className="server-form" onSubmit={handleSubmit}>
          <div className="modal-body">
            {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
            <label className="form-field">
              <span className="form-label">{t("filesPage.archiveName")}</span>
              <input className="form-input" autoFocus value={name} onChange={(e) => setName(e.target.value)} />
            </label>
            <p className="form-note">{t("filesPage.compressNote", { count: targets.length })}</p>
            <div className="form-actions">
              <Button type="button" variant="secondary" onClick={onClose} disabled={busy}>
                {t("common.cancel")}
              </Button>
              <Button type="submit" disabled={busy || !name.trim()}>
                {busy ? t("common.loading") : t("common.create")}
              </Button>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}
