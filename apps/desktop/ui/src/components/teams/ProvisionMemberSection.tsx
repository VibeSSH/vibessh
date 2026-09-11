import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { copyToClipboard } from "@/utils/copyToClipboard";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { Select } from "@/components/ui/Select";
import { IconButton } from "@/components/ui/IconButton";
import { cloudListRoles, cloudProvisionMember } from "@/services/cloudService";
import { toastSuccess } from "@/stores/toastStore";
import type { CloudProvisionedMember, CloudRoleWithPermissions } from "@/types/cloud";
import { errorMessage } from "@/services/tauri";
import "@/components/servers/forms.css";
import "./ProvisionMemberSection.css";

/**
 * Creating an account for somebody, rather than asking them to make one.
 *
 * The one way to bring somebody in. It replaced an invitation flow that
 * could only reach a person who had already registered on their own -
 * backwards for the case it existed for. Here a lead types an email, picks
 * the role, and gets a password to pass on; the account is already in the
 * team and already able to do its job.
 *
 * The password is shown exactly once. Nothing stores it in the clear and no
 * request can produce it again, so the panel that displays it says so
 * plainly and keeps it on screen until it is dismissed deliberately - a
 * toast that vanished on its own would lose the one copy that exists.
 */
export function ProvisionMemberSection({ teamId, canAdd }: { teamId: string; canAdd: boolean }) {
  const { t } = useTranslation();
  const [email, setEmail] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [roleId, setRoleId] = useState("");
  const [roles, setRoles] = useState<CloudRoleWithPermissions[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [created, setCreated] = useState<CloudProvisionedMember | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    cloudListRoles(teamId)
      .then(setRoles)
      .catch(() => setRoles([]));
  }, [teamId]);

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (!email.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      const result = await cloudProvisionMember(teamId, email.trim(), displayName.trim() || null, roleId || null);
      setCreated(result);
      setCopied(false);
      setEmail("");
      setDisplayName("");
      toastSuccess(t("provisionMember.createdToast", { email: result.user.email }));
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  async function copyPassword() {
    if (!created) return;
    // The inline "copied" label stays as well as the toast: this one is a
    // password shown once, and the label is still on screen a minute later
    // when a toast has long gone.
    if (await copyToClipboard(created.temporaryPassword, { copied: t("common.copied"), failed: t("common.copyFailed") })) {
      setCopied(true);
    }
  }

  if (!canAdd) return null;

  return (
    <Card title={t("provisionMember.title")} subtitle={t("provisionMember.subtitle")}>
      {created && (
        <div className="provision-result">
          <div className="provision-result-header">
            <Icon name="key" size={14} />
            <p className="provision-result-title">{t("provisionMember.resultTitle", { email: created.user.email })}</p>
            <IconButton icon="x" size="sm" title={t("common.close")} onClick={() => setCreated(null)} />
          </div>
          <p className="provision-result-note">{t("provisionMember.resultNote")}</p>
          <div className="provision-password-row">
            {/* Selectable text, not a masked field: this has to be readable
                to be passed on, and it is already known to whoever is
                looking at this screen. */}
            <code className="provision-password">{created.temporaryPassword}</code>
            <Button variant="secondary" size="sm" onClick={copyPassword}>
              <Icon name="copy" size={14} />
              {copied ? t("provisionMember.copied") : t("provisionMember.copy")}
            </Button>
          </div>
          {!created.roleAssigned && <p className="form-note form-note-danger">{t("provisionMember.noRoleWarning")}</p>}
        </div>
      )}

      <form className="provision-form" onSubmit={handleSubmit}>
        <div className="form-row">
          <label className="form-field form-field-grow">
            <span className="form-label">{t("provisionMember.email")}</span>
            <input
              className="form-input"
              type="email"
              value={email}
              onChange={(event) => setEmail(event.target.value)}
              placeholder={t("provisionMember.emailPlaceholder")}
            />
          </label>
          <label className="form-field form-field-grow">
            <span className="form-label">{t("provisionMember.displayName")}</span>
            <input
              className="form-input"
              value={displayName}
              onChange={(event) => setDisplayName(event.target.value)}
              placeholder={t("provisionMember.displayNamePlaceholder")}
            />
          </label>
        </div>

        <div className="form-row">
          <label className="form-field form-field-grow">
            <span className="form-label">{t("provisionMember.role")}</span>
            <Select
              value={roleId}
              onChange={setRoleId}
              items={[{ value: "", label: t("provisionMember.noRole") }, ...roles.map((role) => ({ value: role.id, label: role.name }))]}
            />
          </label>
          <div className="form-actions provision-submit">
            <Button type="submit" disabled={busy || !email.trim()}>
              <Icon name="plus" size={14} />
              {busy ? t("common.saving") : t("provisionMember.submit")}
            </Button>
          </div>
        </div>

        <p className="form-note">{t("provisionMember.forcedChangeNote")}</p>
        {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
      </form>
    </Card>
  );
}
