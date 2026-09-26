import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Checkbox } from "@/components/ui/Checkbox";
import { Dialog } from "@/components/ui/Dialog";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { cloudTwoFactorDisable, cloudTwoFactorEnable, cloudTwoFactorSetup, type TwoFactorSetup } from "@/services/cloudService";
import { errorMessage } from "@/services/tauri";
import { useAuthStore } from "@/stores/authStore";
import { toastSuccess } from "@/stores/toastStore";
import "./AccountSecurityCard.css";

/**
 * The VibeSSH account's own security - for now, two-factor sign-in.
 *
 * Only shown while signed in: it is about the account, not this computer.
 */
export function AccountSecurityCard() {
  const { t } = useTranslation();
  const user = useAuthStore((state) => state.user);
  const [dialog, setDialog] = useState<"enable" | "disable" | null>(null);

  if (!user) return null;
  const enabled = user.twoFactorEnabled === true;

  return (
    <Card
      title={t("accountSecurity.title")}
      subtitle={t("accountSecurity.subtitle", { email: user.email })}
      actions={
        enabled ? (
          <Button variant="secondary" size="sm" onClick={() => setDialog("disable")}>
            {t("accountSecurity.disable")}
          </Button>
        ) : (
          <Button size="sm" onClick={() => setDialog("enable")}>
            <Icon name="shield" size={14} />
            {t("accountSecurity.enable")}
          </Button>
        )
      }
    >
      <div className="account-security-row">
        <span className="account-security-label">{t("accountSecurity.twoFactor")}</span>
        <Badge tone={enabled ? "success" : "neutral"}>{enabled ? t("accountSecurity.on") : t("accountSecurity.off")}</Badge>
      </div>
      <p className="form-note">{enabled ? t("accountSecurity.onNote") : t("accountSecurity.offNote")}</p>

      {dialog === "enable" && <EnableDialog onClose={() => setDialog(null)} />}
      {dialog === "disable" && <DisableDialog onClose={() => setDialog(null)} />}
    </Card>
  );
}

/**
 * Three steps: scan the code, confirm with the first code from the app, then
 * keep the recovery codes. Closing before the last step leaves two-factor off
 * - nothing is switched on until a code has proved the app has the secret.
 */
function EnableDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const user = useAuthStore((state) => state.user);
  const setUser = useAuthStore((state) => state.setUser);
  const [setup, setSetup] = useState<TwoFactorSetup | null>(null);
  const [code, setCode] = useState("");
  const [recoveryCodes, setRecoveryCodes] = useState<string[] | null>(null);
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function start() {
    setBusy(true);
    setError(null);
    try {
      setSetup(await cloudTwoFactorSetup());
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  async function confirm(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const result = await cloudTwoFactorEnable(code.trim());
      setRecoveryCodes(result.recoveryCodes);
      if (user) setUser({ ...user, twoFactorEnabled: true });
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  const copy = (text: string, message: string) =>
    navigator.clipboard
      .writeText(text)
      .then(() => toastSuccess(message))
      .catch((err) => console.warn("copying failed", err));

  // The codes are the only way back in without the phone; the dialog does not
  // close on Escape while they are on screen and not yet confirmed as kept.
  const mustKeepCodes = recoveryCodes !== null && !saved;

  return (
    <Dialog open onClose={mustKeepCodes ? () => undefined : onClose} dismissable={!busy && !mustKeepCodes} size="sm" title={t("accountSecurity.enableTitle")}>
      <div className="modal-body account-security-dialog">
        {!setup && !recoveryCodes && (
          <>
            <p className="dialog-body-text">{t("accountSecurity.intro")}</p>
            {error && <p className="form-note form-note-danger">{error}</p>}
            <div className="form-actions">
              <Button variant="secondary" onClick={onClose} disabled={busy}>
                {t("common.cancel")}
              </Button>
              <Button onClick={() => void start()} disabled={busy}>
                {busy ? t("common.loading") : t("accountSecurity.begin")}
              </Button>
            </div>
          </>
        )}

        {setup && !recoveryCodes && (
          <form className="server-form" onSubmit={confirm}>
            <p className="dialog-body-text">{t("accountSecurity.scan")}</p>
            {/* Drawn by this app from the otpauth link - no QR service sees the secret. */}
            <div className="account-security-qr" dangerouslySetInnerHTML={{ __html: setup.qrSvg }} />
            <div className="account-security-secret">
              <span className="form-note">{t("accountSecurity.manual")}</span>
              <code>{setup.secret.match(/.{1,4}/g)?.join(" ")}</code>
              <IconButton icon="copy" size="sm" title={t("common.copy")} onClick={() => void copy(setup.secret, t("accountSecurity.secretCopied"))} />
            </div>
            <label className="form-field">
              <span className="form-label">{t("accountSecurity.confirmCode")}</span>
              <input
                className="form-input account-security-code"
                value={code}
                onChange={(event) => setCode(event.target.value)}
                inputMode="numeric"
                autoComplete="one-time-code"
                placeholder="123456"
                autoFocus
                required
              />
            </label>
            {error && <p className="form-note form-note-danger">{error}</p>}
            <div className="form-actions">
              <Button type="button" variant="secondary" onClick={onClose} disabled={busy}>
                {t("common.cancel")}
              </Button>
              <Button type="submit" disabled={busy || code.trim().length < 6}>
                {busy ? t("common.loading") : t("accountSecurity.turnOn")}
              </Button>
            </div>
          </form>
        )}

        {recoveryCodes && (
          <>
            <p className="dialog-body-text">{t("accountSecurity.recoveryIntro")}</p>
            <ol className="account-security-codes">
              {recoveryCodes.map((recovery) => (
                <li key={recovery}>
                  <code>{recovery}</code>
                </li>
              ))}
            </ol>
            <Button variant="secondary" size="sm" onClick={() => void copy(recoveryCodes.join("\n"), t("accountSecurity.codesCopied"))}>
              <Icon name="copy" size={14} />
              {t("accountSecurity.copyCodes")}
            </Button>
            <Checkbox checked={saved} onChange={setSaved} label={t("accountSecurity.savedCodes")} />
            <div className="form-actions">
              <Button onClick={onClose} disabled={!saved}>
                {t("accountSecurity.done")}
              </Button>
            </div>
          </>
        )}
      </div>
    </Dialog>
  );
}

function DisableDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const setUser = useAuthStore((state) => state.setUser);
  const [password, setPassword] = useState("");
  const [kind, setKind] = useState<"totp" | "recovery">("totp");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const user = await cloudTwoFactorDisable(password, kind === "totp" ? { totpCode: code.trim() } : { recoveryCode: code.trim() });
      setUser(user);
      toastSuccess(t("accountSecurity.disabledToast"));
      onClose();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open onClose={onClose} dismissable={!busy} size="sm" title={t("accountSecurity.disableTitle")}>
      <form className="modal-body server-form" onSubmit={submit}>
        <p className="dialog-body-text">{t("accountSecurity.disableBody")}</p>
        <label className="form-field">
          <span className="form-label">{t("auth.password")}</span>
          <input className="form-input" type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoFocus required />
        </label>
        <label className="form-field">
          <span className="form-label">{kind === "totp" ? t("auth.twoFactorCode") : t("auth.recoveryCode")}</span>
          <input
            className="form-input"
            value={code}
            onChange={(event) => setCode(event.target.value)}
            inputMode={kind === "totp" ? "numeric" : "text"}
            autoComplete="one-time-code"
            required
          />
        </label>
        <button
          type="button"
          className="form-note-link"
          onClick={() => {
            setKind(kind === "totp" ? "recovery" : "totp");
            setCode("");
          }}
        >
          {kind === "totp" ? t("auth.useRecoveryCode") : t("auth.useAppCode")}
        </button>
        {error && <p className="form-note form-note-danger">{error}</p>}
        <div className="form-actions">
          <Button type="button" variant="secondary" onClick={onClose} disabled={busy}>
            {t("common.cancel")}
          </Button>
          <Button type="submit" variant="danger" disabled={busy}>
            {busy ? t("common.loading") : t("accountSecurity.disable")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
