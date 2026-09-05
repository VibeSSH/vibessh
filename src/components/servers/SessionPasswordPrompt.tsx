import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { useSessionPasswordStore } from "@/stores/sessionPasswordStore";

/**
 * Asks for a Node's password when there is nowhere to have kept it.
 *
 * **When this appears.** VibeSSH keeps passwords in the OS credential store
 * and refuses to write them anywhere else. On a machine with no working
 * Secret Service - which is a lot of Linux installs that are not Ubuntu -
 * there is nothing to keep it in, and password authentication used to be
 * simply unavailable there. This is the third option: hold it in memory for
 * this run, ask again next time, write nothing.
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
  const [password, setPassword] = useState("");

  // Cleared between prompts rather than left holding the previous Node's
  // password in a field somebody might submit without looking.
  useEffect(() => {
    setPassword("");
  }, [pending?.serverId]);

  if (!pending) return null;

  return (
    <Dialog open onClose={cancel} title={t("sessionPassword.title")} size="sm">
      <div className="modal-body">
        <p className="dialog-body-text">{t("sessionPassword.body")}</p>
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
              know where it is about to go, and the answer here - nowhere -
              is unusual enough to be worth stating. */}
          <p className="form-hint">{t("sessionPassword.note")}</p>
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
