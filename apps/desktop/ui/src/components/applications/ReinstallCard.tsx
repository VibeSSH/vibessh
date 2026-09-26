import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Checkbox } from "@/components/ui/Checkbox";
import { Dialog } from "@/components/ui/Dialog";
import { Icon } from "@/components/ui/Icon";
import { reinstallApplication } from "@/services/applicationService";
import { errorMessage } from "@/services/tauri";
import { toastSuccess } from "@/stores/toastStore";

/**
 * Runs the application's installation again - see the backend's
 * `reinstall_application`. For when a server is broken in a way a restart
 * does not fix: a corrupted jar, a botched manual update, an image that has
 * moved on.
 */
export function ReinstallCard({ applicationId, applicationName, onDone }: { applicationId: string; applicationName: string; onDone: () => void }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [wipe, setWipe] = useState(false);
  const [confirmName, setConfirmName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Wiping deletes worlds and configs for good, so it asks for the name to be
  // typed, not just a second click.
  const canConfirm = !wipe || confirmName.trim() === applicationName;

  async function reinstall() {
    setBusy(true);
    setError(null);
    try {
      await reinstallApplication(applicationId, wipe);
      toastSuccess(t("reinstall.doneToast", { name: applicationName }));
      setOpen(false);
      setWipe(false);
      setConfirmName("");
      onDone();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card
      title={t("reinstall.title")}
      subtitle={t("reinstall.subtitle")}
      actions={
        <Button variant="secondary" size="sm" onClick={() => setOpen(true)}>
          <Icon name="refresh-cw" size={14} />
          {t("reinstall.button")}
        </Button>
      }
    >
      <p className="form-note">{t("reinstall.note")}</p>

      {open && (
        <Dialog open onClose={() => setOpen(false)} dismissable={!busy} size="sm" title={t("reinstall.confirmTitle", { name: applicationName })}>
          <div className="modal-body server-form">
            <p className="dialog-body-text">{t("reinstall.confirmBody")}</p>
            <Checkbox checked={wipe} onChange={setWipe} label={t("reinstall.wipe")} disabled={busy} />
            {wipe && (
              <>
                <p className="form-note form-note-danger">{t("reinstall.wipeWarning")}</p>
                <label className="form-field">
                  <span className="form-label">{t("reinstall.typeName", { name: applicationName })}</span>
                  <input className="form-input" value={confirmName} onChange={(event) => setConfirmName(event.target.value)} disabled={busy} />
                </label>
              </>
            )}
            {busy && <p className="form-note">{t("reinstall.busy")}</p>}
            {error && <p className="form-note form-note-danger">{error}</p>}
            <div className="form-actions">
              <Button variant="secondary" onClick={() => setOpen(false)} disabled={busy}>
                {t("common.cancel")}
              </Button>
              <Button variant="danger" onClick={() => void reinstall()} disabled={busy || !canConfirm}>
                {busy ? t("reinstall.working") : t("reinstall.confirm")}
              </Button>
            </div>
          </div>
        </Dialog>
      )}
    </Card>
  );
}
