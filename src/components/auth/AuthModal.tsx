import { FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { useAuthModalStore } from "@/stores/authModalStore";
import { useAuthStore } from "@/stores/authStore";
import { useModalDialog } from "@/hooks/useModalDialog";
import { cloudLogin, cloudRegister } from "@/services/cloudService";
import { toastSuccess } from "@/stores/toastStore";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";
import { errorMessage } from "@/services/tauri";

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

  function reset() {
    setEmail("");
    setPassword("");
    setDisplayName("");
    setError(null);
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
      const user = tab === "login" ? await cloudLogin(email, password) : await cloudRegister(email, password, displayName);
      setUser(user);
      toastSuccess(t(tab === "login" ? "auth.loggedInToast" : "auth.registeredToast", { name: user.displayName }));
      reset();
      close();
    } catch (err) {
      setError(errorMessage(err, t));
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
          <form className="server-form" onSubmit={handleSubmit}>
            {tab === "register" && (
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
