import { FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { useBackdropClose } from "@/hooks/useBackdropClose";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";
import { errorMessage } from "@/services/tauri";

interface ChmodModalProps {
  fileName: string;
  currentMode?: number;
  onClose: () => void;
  onConfirm: (mode: number) => Promise<void>;
}

function toOctal(mode: number): string {
  return (mode & 0o777).toString(8).padStart(3, "0");
}

function toSymbolic(mode: number): string {
  const bits = "rwxrwxrwx".split("");
  return bits.map((bit, i) => (mode & (1 << (8 - i)) ? bit : "-")).join("");
}

/** design brief section 118 - a raw octal input (how anyone who'd reach for "chmod" already thinks about it) with a live rwxrwxrwx preview, not a checkbox grid. Only offered where the provider actually has a POSIX permission concept (LocalApplicationFileProvider on Windows rejects set_permissions outright - see that provider's own doc comment). */
export function ChmodModal({ fileName, currentMode, onClose, onConfirm }: ChmodModalProps) {
  const { t } = useTranslation();
  const backdrop = useBackdropClose(onClose);
  const [octal, setOctal] = useState(currentMode !== undefined ? toOctal(currentMode) : "755");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const parsed = /^[0-7]{3,4}$/.test(octal) ? parseInt(octal, 8) : null;

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (parsed === null) return;
    setBusy(true);
    setError(null);
    try {
      await onConfirm(parsed);
      onClose();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" {...backdrop}>
      <div className="modal-panel modal-panel-sm" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2 className="modal-title">{t("applicationFilesTab.chmodTitle", { name: fileName })}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <form className="server-form" onSubmit={handleSubmit}>
          <div className="modal-body">
            {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
            <label className="form-field">
              <span className="form-label">{t("applicationFilesTab.permissions")}</span>
              <input className="form-input" value={octal} onChange={(e) => setOctal(e.target.value.replace(/[^0-7]/g, "").slice(0, 4))} autoFocus />
            </label>
            <p className="form-note">{parsed !== null ? toSymbolic(parsed) : t("applicationFilesTab.chmodInvalid")}</p>
            <div className="form-actions">
              <Button type="button" variant="secondary" onClick={onClose} disabled={busy}>
                {t("common.cancel")}
              </Button>
              <Button type="submit" disabled={busy || parsed === null}>
                {busy ? t("common.loading") : t("common.save")}
              </Button>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}
