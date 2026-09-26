import { FormEvent, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { useAuthModalStore } from "@/stores/authModalStore";
import { useAuthStore } from "@/stores/authStore";
import { useModalDialog } from "@/hooks/useModalDialog";
import { cloudBackendIsConfigured, cloudLogin, cloudRegister, cloudPublishThisDevice } from "@/services/cloudService";
import { toastSuccess } from "@/stores/toastStore";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";
import { CommandError, errorMessage } from "@/services/tauri";

type Tab = "login" | "register";

export function AuthModal() {
  const { t } = useTranslation();
  const isOpen = useAuthModalStore((s) => s.isOpen);
  const close = useAuthModalStore((s) => s.close);
  const setUser = useAuthStore((s) => s.setUser);

  const [tab, setTab] = useState<Tab>("login");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** Set once the password was right and the account wants its second
   *  factor: which kind is being typed, and what. The email and password stay
   *  as they were, since the sign-in is sent again with them. */
  const [secondFactor, setSecondFactor] = useState<"totp" | "recovery" | null>(null);
  const [code, setCode] = useState("");
  /**
   * Whether an account backend has been chosen at all.
   *
   * `null` while it is being read - neither state is worth flashing a
   * warning for. This is the one place the warning belongs: somebody
   * opening this dialog is about to type an email and a password into a
   * form that cannot possibly work, and telling them afterwards, as a
   * failed request, is telling them too late.
   */
  const [configured, setConfigured] = useState<boolean | null>(null);
  useEffect(() => {
    cloudBackendIsConfigured()
      .then(setConfigured)
      .catch(() => undefined);
  }, []);

  function reset() {
    setEmail("");
    setPassword("");
    setDisplayName("");
    setError(null);
    setSecondFactor(null);
    setCode("");
  }

  function handleClose() {
    reset();
    close();
  }

  const backdrop = useModalDialog(handleClose, { labelledBy: "authmodal-dialog-title-1" });

  if (!isOpen) return null;

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const factor = secondFactor === "totp" ? { totpCode: code } : secondFactor === "recovery" ? { recoveryCode: code } : undefined;
      const user = tab === "login" ? await cloudLogin(email, password, factor) : await cloudRegister(email, password, displayName);
      setUser(user);
      // Registers this machine's public key, so a teammate's install can put
      // it in the account it creates for this person on a shared Node.
      // Deliberately not awaited into the success path: signing in worked,
      // and a backend that is briefly unreachable must not turn that into a
      // failure. The next sign-in publishes again - the backend treats the
      // same key twice as the same device.
      cloudPublishThisDevice().catch((err) => console.warn("couldn't publish this device's key", err));
      toastSuccess(t(tab === "login" ? "auth.loggedInToast" : "auth.registeredToast", { name: user.displayName }));
      reset();
      close();
    } catch (err) {
      // The password was right and the account has two-factor on: ask for
      // the code rather than show it as a failure.
      if (err instanceof CommandError && err.params.backendCode === "two_factor_required") {
        setSecondFactor("totp");
        setError(null);
      } else {
        setError(errorMessage(err, t));
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" {...backdrop.backdropProps}>
      <div className="modal-panel" {...backdrop.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="authmodal-dialog-title-1">{t("auth.modalTitle")}</h2>
          <IconButton icon="x" size="sm" onClick={handleClose} title={t("common.close")} />
        </div>

        <div className="modal-tabs">
          <button className={`modal-tab ${tab === "login" ? "modal-tab-active" : ""}`} onClick={() => setTab("login")}>
            {t("auth.tabLogin")}
          </button>
          <button className={`modal-tab ${tab === "register" ? "modal-tab-active" : ""}`} onClick={() => setTab("register")}>
            {t("auth.tabRegister")}
          </button>
        </div>

        <div className="modal-body">
          {/* Before the fields, not after a failed request: the form cannot
              work without a backend, and the remedy is somewhere else
              entirely. */}
          {configured === false && (
            <p className="form-note form-note-danger form-note-spaced">{t("auth.noBackend")}</p>
          )}
          <form className="server-form" onSubmit={handleSubmit}>
            {secondFactor && (
              <>
                <p className="dialog-body-text">{secondFactor === "totp" ? t("auth.twoFactorPrompt") : t("auth.recoveryPrompt")}</p>
                <label className="form-field">
                  <span className="form-label">{secondFactor === "totp" ? t("auth.twoFactorCode") : t("auth.recoveryCode")}</span>
                  <input
                    className="form-input"
                    value={code}
                    onChange={(e) => setCode(e.target.value)}
                    inputMode={secondFactor === "totp" ? "numeric" : "text"}
                    autoComplete="one-time-code"
                    placeholder={secondFactor === "totp" ? "123456" : "abcde-fghjk"}
                    autoFocus
                    required
                  />
                </label>
                <button
                  type="button"
                  className="form-note-link"
                  onClick={() => {
                    setSecondFactor(secondFactor === "totp" ? "recovery" : "totp");
                    setCode("");
                    setError(null);
                  }}
                >
                  {secondFactor === "totp" ? t("auth.useRecoveryCode") : t("auth.useAppCode")}
                </button>
              </>
            )}
            {!secondFactor && tab === "register" && (
              <label className="form-field">
                <span className="form-label">{t("auth.displayName")}</span>
                <input
                  className="form-input"
                  placeholder={t("auth.displayNamePlaceholder")}
                  value={displayName}
                  onChange={(e) => setDisplayName(e.target.value)}
                  required
                />
              </label>
            )}
            {!secondFactor && (
              <>
            <label className="form-field">
              <span className="form-label">{t("auth.email")}</span>
              <input
                className="form-input"
                type="email"
                placeholder={t("auth.emailPlaceholder")}
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                required
              />
            </label>
            <label className="form-field">
              <span className="form-label">{t("auth.password")}</span>
              <input
                className="form-input"
                type="password"
                placeholder="••••••••"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
              />
            </label>
              </>
            )}

            {error && <p className="form-note form-note-danger">{error}</p>}

            <div className="form-actions">
              <Button type="submit" disabled={busy}>
                {busy ? t("common.loading") : t(tab === "login" ? "auth.tabLogin" : "auth.tabRegister")}
              </Button>
            </div>
            <p className="form-note">{t("auth.backendNote")}</p>
          </form>
        </div>
      </div>
    </div>
  );
}
