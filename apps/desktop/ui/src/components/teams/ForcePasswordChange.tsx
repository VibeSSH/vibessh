import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { cloudChangePassword, cloudLogout } from "@/services/cloudService";
import { useAuthStore } from "@/stores/authStore";
import { errorMessage } from "@/services/tauri";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";
import "./ForcePasswordChange.css";

/**
 * The screen an account gets when its password was set by somebody else.
 *
 * It has no close button and no backdrop dismissal, which is unusual here
 * and deliberate: the account is in a state where the backend refuses
 * everything except reading its own profile and setting a password, so a
 * dismissible dialog would leave somebody looking at an application in
 * which nothing works, with no explanation. The only two ways out are the
 * two that lead somewhere - set a password, or sign out.
 *
 * This is not the enforcement. The backend refuses regardless of what any
 * client does; this exists so the refusal is met with the one screen that
 * resolves it rather than with a string of failures.
 */
export function ForcePasswordChange() {
  const { t } = useTranslation();
  const user = useAuthStore((state) => state.user);
  const setUser = useAuthStore((state) => state.setUser);

  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Checked here as well as by the backend, because getting told "they do
  // not match" after a round trip is a worse way to learn it.
  const mismatch = confirm !== "" && next !== confirm;
  const tooShort = next !== "" && next.length < 8;
  const canSubmit = current !== "" && next !== "" && !mismatch && !tooShort && !busy;

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (!canSubmit) return;
    setBusy(true);
    setError(null);
    try {
      const updated = await cloudChangePassword(current, next);
      // The profile that comes back has the flag cleared, which is what
      // takes this screen away.
      setUser(updated);
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  async function handleSignOut() {
    try {
      await cloudLogout();
    } finally {
      setUser(null);
    }
  }

  return (
    <div className="modal-backdrop force-password-backdrop" role="dialog" aria-modal="true" aria-labelledby="force-password-title">
      <div className="modal-panel modal-panel-sm">
        <div className="modal-header">
          <h2 className="modal-title" id="force-password-title">
            {t("forcePassword.title")}
          </h2>
        </div>
        <form className="server-form" onSubmit={handleSubmit}>
          <div className="modal-body">
            <p className="form-note force-password-intro">{t("forcePassword.body", { email: user?.email ?? "" })}</p>

            <label className="form-field">
              <span className="form-label">{t("forcePassword.current")}</span>
              <input
                className="form-input"
                type="password"
                value={current}
                onChange={(event) => setCurrent(event.target.value)}
                autoComplete="current-password"
                autoFocus
              />
            </label>

            <label className="form-field">
              <span className="form-label">{t("forcePassword.next")}</span>
              <input
                className="form-input"
                type="password"
                value={next}
                onChange={(event) => setNext(event.target.value)}
                autoComplete="new-password"
              />
              {tooShort && <span className="form-note form-note-danger">{t("forcePassword.tooShort")}</span>}
            </label>

            <label className="form-field">
              <span className="form-label">{t("forcePassword.confirm")}</span>
              <input
                className="form-input"
                type="password"
                value={confirm}
                onChange={(event) => setConfirm(event.target.value)}
                autoComplete="new-password"
              />
              {mismatch && <span className="form-note form-note-danger">{t("forcePassword.mismatch")}</span>}
            </label>

            {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}

            <div className="form-actions">
              <Button type="button" variant="secondary" onClick={handleSignOut} disabled={busy}>
                {t("forcePassword.signOut")}
              </Button>
              <Button type="submit" disabled={!canSubmit}>
                {busy ? t("common.saving") : t("forcePassword.submit")}
              </Button>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}
