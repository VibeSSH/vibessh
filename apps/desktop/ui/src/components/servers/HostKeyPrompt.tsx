import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { useHostKeyStore } from "@/stores/hostKeyStore";
import { toastSuccess } from "@/stores/toastStore";
import "./HostKeyPrompt.css";

/** What to run on the Node to see its own fingerprints, to compare with the one shown. */
const VERIFY_COMMAND = "for f in /etc/ssh/ssh_host_*_key.pub; do ssh-keygen -lf \"$f\"; done";

/**
 * The warning for a Node whose SSH host key changed, and the only way to
 * accept the new one.
 *
 * A changed key is either a reinstalled server or somebody in the middle of
 * the connection, and only the person can tell which - so the dialog shows
 * both fingerprints and the command that prints the Node's own, and trusting
 * is a deliberate second choice beside the safe default of not connecting.
 */
export function HostKeyPrompt() {
  const { t } = useTranslation();
  const pending = useHostKeyStore((state) => state.pending);
  const answer = useHostKeyStore((state) => state.answer);
  const [confirming, setConfirming] = useState(false);

  if (!pending) return null;

  const close = (trusted: boolean) => {
    setConfirming(false);
    answer(trusted);
  };

  return (
    <Dialog open onClose={() => close(false)} size="md" title={t("hostKey.title")}>
      <div className="modal-body host-key">
        <p className="host-key-lead">
          <Icon name="alert-triangle" size={16} />
          {t("hostKey.lead", { host: pending.host })}
        </p>
        <p className="dialog-body-text">{t("hostKey.body")}</p>

        <dl className="host-key-prints">
          {pending.expected && (
            <>
              <dt>{t("hostKey.expected")}</dt>
              <dd>{pending.expected}</dd>
            </>
          )}
          <dt>{t("hostKey.presented")}</dt>
          <dd className="host-key-new">{pending.presented}</dd>
        </dl>

        <p className="form-note">{t("hostKey.verify")}</p>
        <div className="host-key-command">
          <code>{VERIFY_COMMAND}</code>
          <IconButton
            icon="copy"
            size="sm"
            title={t("common.copy")}
            onClick={() => {
              void navigator.clipboard
                .writeText(VERIFY_COMMAND)
                .then(() => toastSuccess(t("hostKey.copied")))
                .catch((err) => console.warn("copying the verify command failed", err));
            }}
          />
        </div>

        {confirming && <p className="form-note form-note-danger">{t("hostKey.confirmNote")}</p>}

        <div className="form-actions">
          <Button variant="secondary" onClick={() => close(false)}>
            {t("hostKey.dontConnect")}
          </Button>
          {confirming ? (
            <Button variant="danger" onClick={() => close(true)}>
              {t("hostKey.trustConfirm")}
            </Button>
          ) : (
            <Button variant="secondary" onClick={() => setConfirming(true)}>
              {t("hostKey.trust")}
            </Button>
          )}
        </div>
      </div>
    </Dialog>
  );
}
