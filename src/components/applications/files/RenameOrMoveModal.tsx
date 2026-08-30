import { FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { useBackdropClose } from "@/hooks/useBackdropClose";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";

interface RenameOrMoveModalProps {
  mode: "rename" | "move" | "copy";
  currentPath: string;
  currentName: string;
  onClose: () => void;
  /** `to` is a full path relative to the application root - the caller builds it from the current directory + the new name (rename) or the typed destination directory (move). */
  onConfirm: (to: string) => Promise<void>;
}

const TITLE_KEY = { rename: "applicationFilesTab.renameTitle", move: "applicationFilesTab.moveTitle", copy: "applicationFilesTab.copyTitle" } as const;
const ERROR_KEY = { rename: "applicationFilesTab.renameError", move: "applicationFilesTab.moveError", copy: "applicationFilesTab.copyError" } as const;

/** Covers "Rename", "Move", and "Copy" (design brief sections 108/121) - Rename/Move both call the same backend `rename` primitive with a different destination path (the caller's `onConfirm` decides which), matching how the actual filesystem/SFTP operation is identical either way; Copy's `onConfirm` calls the separate `copy` primitive instead, but shares this exact same "type a destination path" UI. */
export function RenameOrMoveModal({ mode, currentPath, currentName, onClose, onConfirm }: RenameOrMoveModalProps) {
  const { t } = useTranslation();
  const backdrop = useBackdropClose(onClose);
  const parentDir = currentPath.includes("/") ? currentPath.slice(0, currentPath.lastIndexOf("/")) : "";
  const [name, setName] = useState(mode === "rename" ? currentName : "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    const trimmed = name.trim();
    if (!trimmed) return;
    const to = mode === "rename" ? (parentDir ? `${parentDir}/${trimmed}` : trimmed) : trimmed.replace(/^\/+/, "");
    setBusy(true);
    setError(null);
    try {
      await onConfirm(to);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : t(ERROR_KEY[mode]));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" {...backdrop}>
      <div className="modal-panel modal-panel-sm" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2 className="modal-title">{t(TITLE_KEY[mode])}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <form className="server-form" onSubmit={handleSubmit}>
          <div className="modal-body">
            {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
            <label className="form-field">
              <span className="form-label">{t(mode === "rename" ? "applicationFilesTab.newName" : "applicationFilesTab.destinationPath")}</span>
              <input
                className="form-input"
                autoFocus
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={mode !== "rename" ? t("applicationFilesTab.destinationPathPlaceholder") : undefined}
              />
            </label>
            {mode !== "rename" && <p className="form-note">{t("applicationFilesTab.moveNote")}</p>}
            <div className="form-actions">
              <Button type="button" variant="secondary" onClick={onClose} disabled={busy}>
                {t("common.cancel")}
              </Button>
              <Button type="submit" disabled={busy || !name.trim()}>
                {busy ? t("common.loading") : t("common.save")}
              </Button>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}
