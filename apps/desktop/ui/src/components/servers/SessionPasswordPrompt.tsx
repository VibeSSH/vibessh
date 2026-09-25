import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { Icon } from "@/components/ui/Icon";
import { useServerModalStore } from "@/stores/serverModalStore";
import { useServersStore } from "@/stores/serversStore";
import { useSessionPasswordStore } from "@/stores/sessionPasswordStore";
import "./SessionPasswordPrompt.css";

/**
 * Asks for a Node's password - because there is nowhere to have kept it, or
 * because the Node refused the one it was given.
 *
 * **Missing.** VibeSSH keeps passwords in the OS credential store and refuses
 * to write them anywhere else. On a machine with no working Secret Service -
 * which is a lot of Linux installs that are not Ubuntu - there is nothing to
 * keep it in. This holds it in memory for this run, asks again next time,
 * writes nothing.
 *
 * **Refused.** A wrong password used to surface as one line of English
 * wherever the failure happened to land - the terminal, a card, nowhere.
 * Now it is asked for here, in the middle of the screen, and the new one is
 * saved where Edit Server would have saved it. A refused *key* cannot be
 * retyped, so that case offers the server's settings instead of a field.
 *
 * Rendered once at the layout level rather than per screen. A connection can
 * be opened from almost anywhere in the app, and a prompt that only existed
 * on the Servers page would leave every other route with a dead end.
 */
export function SessionPasswordPrompt() {
  const { t } = useTranslation();
  const pending = useSessionPasswordStore((state) => state.pending);
  const submit = useSessionPasswordStore((state) => state.submit);
  const cancel = useSessionPasswordStore((state) => state.cancel);
  const server = useServersStore((state) => state.servers.find((s) => s.id === pending?.serverId));
  const openForEdit = useServerModalStore((state) => state.openForEdit);
  const [password, setPassword] = useState("");

  // Cleared between prompts rather than left holding the previous Node's
  // password in a field somebody might submit without looking - and between
  // two refusals of the same Node, so a retry starts from an empty field.
  useEffect(() => {
    setPassword("");
  }, [pending]);

  if (!pending) return null;

  const names = { username: pending.username ?? "", server: server?.name ?? "" };

  if (pending.reason === "rejectedKey") {
    return (
      <Dialog open onClose={cancel} title={t("sessionPassword.rejectedKeyTitle")} size="sm">
        <div className="modal-body">
          <p className="session-password-refused">
            <Icon name="alert-triangle" size={16} />
            <span>{t("sessionPassword.rejectedKeyBody", names)}</span>
          </p>
          <div className="form-actions">
            <Button variant="secondary" type="button" onClick={cancel}>
              {t("common.cancel")}
            </Button>
            {server && (
              <Button
                type="button"
                onClick={() => {
                  cancel();
                  openForEdit(server);
                }}
              >
                {t("sessionPassword.editServer")}
              </Button>
            )}
          </div>
        </div>
      </Dialog>
    );
  }

  const rejected = pending.reason === "rejected";

  return (
    <Dialog open onClose={cancel} title={rejected ? t("sessionPassword.rejectedTitle") : t("sessionPassword.title")} size="sm">
      <div className="modal-body">
        {rejected ? (
          <p className="session-password-refused">
            <Icon name="alert-triangle" size={16} />
            <span>{t("sessionPassword.rejectedBody", names)}</span>
          </p>
        ) : (
          <p className="dialog-body-text">{t("sessionPassword.body")}</p>
        )}
        <form
          onSubmit={(event) => {
            event.preventDefault();
            if (password.length > 0) submit(password);
          }}
        >
          <input
            className="form-input"
            type="password"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            placeholder={t("sessionPassword.placeholder")}
            aria-label={t("sessionPassword.placeholder")}
            autoFocus
          />
          {/* Said before the buttons: somebody typing a password deserves to
              know where it is about to go. */}
          <p className="form-hint">{rejected ? t("sessionPassword.rejectedNote") : t("sessionPassword.note")}</p>
          <div className="form-actions">
            <Button variant="secondary" type="button" onClick={cancel}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={password.length === 0}>
              {t("sessionPassword.connect")}
            </Button>
          </div>
        </form>
      </div>
    </Dialog>
  );
}
