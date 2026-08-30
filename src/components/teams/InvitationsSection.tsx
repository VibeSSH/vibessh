import { FormEvent, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { cloudCreateInvitation, cloudListInvitations, cloudListRoles, cloudRevokeInvitation } from "@/services/cloudService";
import { toastSuccess } from "@/stores/toastStore";
import type { CloudInvitation, CloudRole, InvitationStatus } from "@/types/cloud";
import "@/components/servers/forms.css";
import "./InvitationsSection.css";

const STATUS_TONE: Record<InvitationStatus, "success" | "danger" | "warning" | "neutral"> = {
  pending: "warning",
  accepted: "success",
  declined: "neutral",
  revoked: "neutral",
  expired: "danger",
};

interface InvitationsSectionProps {
  teamId: string;
  canManage: boolean;
}

/**
 * The token a fresh invitation returns is shown here exactly once (matches
 * the backend's own "shown once" rule - see backend/src/models.rs's
 * CreatedInvitation) - there's no email delivery in this backend, so
 * getting that token to the invitee (copy/paste into chat, email, whatever)
 * is on whoever's inviting. The invitee then accepts/declines it from
 * Teams.tsx, since they don't necessarily have this team open yet.
 */
export function InvitationsSection({ teamId, canManage }: InvitationsSectionProps) {
  const { t } = useTranslation();
  const [invitations, setInvitations] = useState<CloudInvitation[]>([]);
  const [roles, setRoles] = useState<CloudRole[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [email, setEmail] = useState("");
  const [roleId, setRoleId] = useState("");
  const [creating, setCreating] = useState(false);
  const [createdToken, setCreatedToken] = useState<{ email: string; token: string } | null>(null);

  function load() {
    setLoading(true);
    setError(null);
    Promise.all([cloudListInvitations(teamId), cloudListRoles(teamId)])
      .then(([loadedInvitations, loadedRoles]) => {
        setInvitations(loadedInvitations);
        setRoles(loadedRoles);
      })
      .catch((err) => setError(err instanceof Error ? err.message : t("invitations.couldntList")))
      .finally(() => setLoading(false));
  }

  useEffect(load, [teamId]); // eslint-disable-line react-hooks/exhaustive-deps

  async function handleCreate(e: FormEvent) {
    e.preventDefault();
    const trimmed = email.trim();
    if (!trimmed) return;
    setCreating(true);
    setError(null);
    try {
      const created = await cloudCreateInvitation(teamId, trimmed, roleId || null, null);
      setCreatedToken({ email: created.email, token: created.token });
      setEmail("");
      setRoleId("");
      toastSuccess(t("invitations.createdToast", { email: created.email }));
      load();
    } catch (err) {
      setError(err instanceof Error ? err.message : t("invitations.couldntCreate"));
    } finally {
      setCreating(false);
    }
  }

  async function handleRevoke(invitation: CloudInvitation) {
    setError(null);
    try {
      await cloudRevokeInvitation(teamId, invitation.id);
      toastSuccess(t("invitations.revokedToast", { email: invitation.email }));
      load();
    } catch (err) {
      setError(err instanceof Error ? err.message : t("invitations.couldntRevoke"));
    }
  }

  async function handleCopyToken() {
    if (!createdToken) return;
    try {
      await navigator.clipboard.writeText(createdToken.token);
    } catch {
      // clipboard access denied - nothing useful to do about it here
    }
  }

  return (
    <Card title={t("invitations.title")} subtitle={t("invitations.subtitle")}>
      {error && <p className="page-error-note">{error}</p>}

      {createdToken && (
        <div className="invitations-token-panel">
          <p className="invitations-token-note">{t("invitations.tokenNote", { email: createdToken.email })}</p>
          <div className="code-block">
            <span className="code-block-text">{createdToken.token}</span>
            <button type="button" className="code-block-copy" onClick={handleCopyToken} aria-label={t("invitations.copyTokenAria")}>
              <Icon name="copy" size={14} />
            </button>
          </div>
          <button type="button" className="invitations-token-dismiss" onClick={() => setCreatedToken(null)}>
            {t("invitations.dismissToken")}
          </button>
        </div>
      )}

      {loading ? (
        <SkeletonRows />
      ) : invitations.length === 0 ? (
        <EmptyState icon="user" title={t("invitations.emptyTitle")} description={t("invitations.emptyDescription")} />
      ) : (
        <ul className="server-list">
          {invitations.map((invitation) => (
            <li key={invitation.id} className="server-list-item">
              <div className="server-list-main">
                <span className="server-list-name" title={invitation.email}>
                  {invitation.email}
                </span>
                <span className="server-list-host">
                  {t("invitations.expiresOn", { date: new Date(invitation.expiresAt).toLocaleDateString() })}
                </span>
              </div>
              <Badge tone={STATUS_TONE[invitation.status]}>{t(`invitations.status.${invitation.status}`)}</Badge>
              {canManage && invitation.status === "pending" && (
                <IconButton
                  icon="trash"
                  size="sm"
                  danger
                  title={t("invitations.revokeAria", { email: invitation.email })}
                  onClick={() => handleRevoke(invitation)}
                />
              )}
            </li>
          ))}
        </ul>
      )}

      {canManage && (
        <form className="invitations-form" onSubmit={handleCreate}>
          <div className="invitations-form-row">
            <input
              className="form-input"
              type="email"
              placeholder={t("invitations.emailPlaceholder")}
              value={email}
              onChange={(e) => setEmail(e.target.value)}
            />
            <select className="form-input" value={roleId} onChange={(e) => setRoleId(e.target.value)}>
              <option value="">{t("invitations.noRole")}</option>
              {roles.map((role) => (
                <option key={role.id} value={role.id}>
                  {role.name}
                </option>
              ))}
            </select>
            <Button type="submit" disabled={creating || !email.trim()}>
              <Icon name="plus" size={14} />
              {creating ? t("common.loading") : t("invitations.invite")}
            </Button>
          </div>
        </form>
      )}
    </Card>
  );
}
